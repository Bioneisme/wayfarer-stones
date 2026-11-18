//! The server half: the state of the open world, its files, menus, commands and travels.

use crate::rules;
use mod_api::{
    Block, BlockChangedEvent, BlockKey, BlockProperties, BlockShapes, Command, CommandArgumentValue,
    CommandSource, Flow, IPayloadContext, IPayloadHandler, Identifier, Item, ItemKey, LevelRef, PayloadBytes,
    LevelDimension, LevelHost, PayloadHandleError, PayloadId, PlayerRef, PushReaction, Rarity, Registrar,
    RunModCommandEvent, ServerHost, Text, text::TextStyle,
};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
};

// ---- keys --------------------------------------------------------------------

/// The registry keys of the mod's blocks and items, looked up once at registration.
pub(crate) struct Keys {
    pub block: BlockKey,
    pub top: BlockKey,
    pub stone_item: ItemKey,
    pub scroll: ItemKey,
}

static KEYS: OnceLock<Keys> = OnceLock::new();

pub(crate) fn keys() -> &'static Keys {
    KEYS.get().expect("the mod's keys are looked up at registration")
}

/// Looks the keys up and registers the mod messages.
pub(crate) fn register(r: &mut dyn Registrar) {
    let keys = Keys {
        block: r.block_key(rules::BLOCK),
        top: r.block_key(rules::TOP),
        stone_item: r.item_key(rules::BLOCK),
        scroll: r.item_key(rules::SCROLL),
    };
    let _ = KEYS.set(keys);
    let mut payloads = r.payload_handlers();
    let mut view = payloads.registrar("1");
    view.play_to_server(PayloadId(rules::CHANNEL_ACT.into()), Arc::new(FromClient));
    view.play_to_client(PayloadId(rules::CHANNEL_LIST.into()), Some(Arc::new(crate::screen::FromServer)));
}

// ---- the world's state ---------------------------------------------------------

/// The mod of the open world.
struct State {
    dir: PathBuf,
    reg: rules::Registry,
    /// Each player's open screen, as far as the server knows.
    sessions: HashMap<u128, rules::Session>,
    /// Screens to open at the end of the tick.
    opens: rules::OpenQueue,
    /// Travels waiting for the next tick (or their warmup).
    pending: Vec<rules::Pending>,
    /// Travels made to chunks not yet loaded: their players wait on the stone.
    settling: Vec<rules::Settling>,
    /// When each player may travel again (ms). Not saved: it is seconds long.
    cooldowns: HashMap<u128, i64>,
    /// Console commands for the end of the tick: a stone's other block, and what a player sees and
    /// hears. They never run inside another hook: a command can fire this mod's hooks again.
    commands: Vec<String>,
    /// Errors reading the files: the commands say so instead of writing over a damaged file.
    broken: Option<String>,
    /// The last save's error, logged once until a save succeeds.
    save_error: Option<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static TICKS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `8-4-4-4-12` hex, as `players.json` keys and the owners are written.
fn uuid_string(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// A JSON document: its default when the file doesn't exist, an error when it exists but can't be
/// read, so a damaged file is never replaced.
fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The world's folder as the rules save into it: a file is staged as `.<name>.tmp` beside it,
/// flushed, and put in place by a rename.
struct Folder<'a>(&'a Path);

impl Folder<'_> {
    fn temp(&self, name: &str) -> PathBuf {
        self.0.join(format!(".{name}.tmp"))
    }
}

impl rules::SaveFiles for Folder<'_> {
    fn stage(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let temp = self.temp(name);
        let write = || -> io::Result<()> {
            fs::create_dir_all(self.0)?;
            let mut file = fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()
        };
        write().map_err(|e| format!("{}: {e}", temp.display()))
    }

    fn commit(&mut self, name: &str) -> Result<(), String> {
        let path = self.0.join(name);
        fs::rename(self.temp(name), &path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Written whole and atomically: a temporary file beside it, flushed, then renamed over the old one.
fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| format!("{}: {e}", path.display()))?;
    bytes.push(b'\n');
    let mut folder = Folder(dir);
    rules::SaveFiles::stage(&mut folder, name, &bytes)?;
    rules::SaveFiles::commit(&mut folder, name)
}

fn open(dir: PathBuf) -> State {
    let mut broken = None;
    let config_path = dir.join("config.json");
    let config = match read_json::<Option<serde_json::Value>>(&config_path) {
        Ok(None) => {
            let c = rules::Config::default();
            if let Err(e) = write_json(&config_path, &c) {
                log::warn!("[wayfarer_stones] {e}");
            }
            c
        }
        Ok(Some(v)) => match serde_json::from_value::<rules::Config>(v.clone()) {
            Ok(c) => {
                // Settings a newer version added, so a player can find them.
                if serde_json::to_value(&c).is_ok_and(|now| now != v)
                    && let Err(e) = write_json(&config_path, &c)
                {
                    log::warn!("[wayfarer_stones] {e}");
                }
                c
            }
            Err(e) => {
                broken = Some(format!("{}: {e}", config_path.display()));
                rules::Config::default()
            }
        },
        Err(e) => {
            broken = Some(e);
            rules::Config::default()
        }
    };
    let data = read_json::<rules::Data>(&dir.join(rules::STONES_FILE)).unwrap_or_else(|e| {
        broken.get_or_insert(e);
        rules::Data::default()
    });
    let players = read_json::<rules::Players>(&dir.join(rules::PLAYERS_FILE)).unwrap_or_else(|e| {
        broken.get_or_insert(e);
        rules::Players::default()
    });
    if let Some(e) = &broken {
        log::error!("[wayfarer_stones] can't read {e}: the mod is read-only until it is fixed");
    } else {
        log::info!("[wayfarer_stones] {} stone(s) in {}", data.stones.len(), dir.display());
    }
    State {
        dir,
        reg: rules::Registry::new(config, data, players),
        sessions: HashMap::new(),
        opens: rules::OpenQueue::default(),
        pending: Vec::new(),
        settling: Vec::new(),
        cooldowns: HashMap::new(),
        commands: Vec::new(),
        broken,
        save_error: None,
    }
}

fn lock() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The state of the world in `dir` (`<world>/horizon-wayfarer-stones/`), opened on first use and
/// again when another world opens.
fn state(dir: PathBuf) -> MutexGuard<'static, Option<State>> {
    let mut state = lock();
    if state.as_ref().is_none_or(|s| s.dir != dir) {
        if let Some(old) = state.as_mut() {
            close(old);
        }
        *state = Some(open(dir));
    }
    state
}

/// Writes what changed. A failed write leaves it changed, so the next save (once a second) tries
/// again; the error is logged once until one succeeds.
fn save(st: &mut State) {
    if st.broken.is_some() {
        // Never write over a file that couldn't be read.
        return;
    }
    match st.reg.save(&mut Folder(&st.dir)) {
        Ok(()) => {
            if st.save_error.take().is_some() {
                log::info!("[wayfarer_stones] saved {} again", st.dir.display());
            }
        }
        Err(e) => {
            if st.save_error.as_ref() != Some(&e) {
                log::warn!("[wayfarer_stones] can't save, will retry: {e}");
            }
            st.save_error = Some(e);
        }
    }
}

/// The world's last save, as its state is dropped: what can't be written now is lost, so it says so.
fn close(st: &mut State) {
    save(st);
    if st.reg.changed && st.broken.is_none() {
        log::error!(
            "[wayfarer_stones] {} closed with changes that could not be saved: {}",
            st.dir.display(),
            st.save_error.as_deref().unwrap_or("unknown error")
        );
    }
}

/// The world closing: the files are written and the state let go.
pub(crate) fn close_world() {
    let mut state = lock();
    if let Some(st) = state.as_mut() {
        close(st);
    }
    *state = None;
}

fn world_state(host: &dyn ServerHost) -> Option<MutexGuard<'static, Option<State>>> {
    let dir = host.world_dir()?;
    Some(state(dir.join(rules::DIR)))
}

// ---- players -------------------------------------------------------------------

/// A player as the rules see them, read once.
#[derive(Clone)]
struct Pl {
    player: PlayerRef,
    uuid: u128,
    name: String,
    level: u8,
    dimension: String,
    position: [f64; 3],
}

fn identifier(id: &Identifier) -> String {
    format!("{}:{}", id.namespace(), id.path())
}

fn pl(host: &dyn ServerHost, player: PlayerRef) -> Option<Pl> {
    Some(Pl {
        player,
        uuid: player.uuid(),
        name: player.name(host)?,
        level: player.permission_level(host).unwrap_or(0),
        dimension: identifier(&player.dimension(host)?),
        position: player.entity().position(host)?,
    })
}

fn by_uuid(host: &dyn ServerHost, uuid: u128) -> Option<Pl> {
    pl(host, host.player(uuid)?)
}

fn actor(p: &Pl) -> rules::Actor {
    rules::Actor {
        uuid: uuid_string(p.uuid),
        name: p.name.clone(),
        level: p.level,
    }
}

// ---- text ----------------------------------------------------------------------

/// A line as a component: it goes to a player's client by its key, and they read it in their language.
pub(crate) fn line(m: &rules::Msg) -> Text {
    let args = m
        .args
        .iter()
        .map(|a| match a {
            rules::Arg::Plain(s) => Text::literal(s.clone()),
            rules::Arg::Nested(n) => line(n),
        })
        .collect();
    Text::translate(m.key.clone(), args)
}

fn colored(m: rules::Msg, color: &str) -> Text {
    let mut style = TextStyle::default();
    style.color = Some(color.to_owned());
    line(&m).with_style(style)
}
fn green(m: rules::Msg) -> Text {
    colored(m, "green")
}
fn red(m: rules::Msg) -> Text {
    colored(m, "red")
}
fn gold(m: rules::Msg) -> Text {
    colored(m, "gold")
}

fn tell(host: &mut dyn ServerHost, player: PlayerRef, line: Text) {
    let _ = player.tell(host, line);
}

// ---- declarations --------------------------------------------------------------

/// A stone's two blocks. A heavy core is their template: a plain block that doesn't hide its
/// neighbours' faces, which a model narrower than a cube needs. A piston neither moves nor crushes
/// them, so it never parts the two. The lower block has the item. Textures, models, the language
/// files, the loot tables, the recipes and the villages are this package's `assets/` and `data/`.
pub(crate) fn blocks(blocks: &mut Vec<Block>) {
    for (id, item) in [(rules::BLOCK, true), (rules::TOP, false)] {
        // Narrower than a block, as tall as one: a player stands on top.
        let shape = vec![[0.125, 0.0, 0.125, 0.875, 1.0, 0.875]];
        let mut block = Block::new(id, "minecraft:heavy_core").with_item(item);
        block.properties = BlockProperties {
            hardness: Some(3.0),
            explosion_resistance: Some(1200.0),
            light: Some(7),
            requires_tool: Some(true),
            sounds: Some("minecraft:lodestone".into()),
            push_reaction: Some(PushReaction::Immoveable),
            ..Default::default()
        };
        block.shapes = Some(BlockShapes {
            properties: Vec::new(),
            values: std::collections::BTreeMap::new(),
            default: Some(shape),
        });
        blocks.push(block);
    }
}

/// The wayfarer's scroll: paper that opens the menu from anywhere and is used up by the travel.
pub(crate) fn items(items: &mut Vec<Item>) {
    let mut scroll = Item::new(rules::SCROLL, "minecraft:paper");
    scroll.max_stack_size = Some(16);
    scroll.rarity = Some(Rarity::Uncommon);
    items.push(scroll);
}

/// The commands in every player's command tree.
pub(crate) fn commands(commands: &mut Vec<Command>) {
    commands.push(Command::new("wayfarer"));
    commands.push(Command::new("wayfarer list"));
    commands.push(Command::new("wayfarer to <name:text>"));
    commands.push(Command::new("wayfarer name <name:text>"));
    commands.push(Command::new("wayfarer forget <name:text>"));
    commands.push(Command::op("wayfarer global <name:text>", 2));
    commands.push(Command::op("wayfarer remove <name:text>", 2));
}

// ---- blocks in the world -------------------------------------------------------

fn block_at(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> Option<BlockKey> {
    level.block(host, pos)
}

/// The lower block of the stone at `pos`, either of its two blocks; none when no stone stands there
/// (a top alone, left by `/setblock`, is none).
fn lower_of(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> Option<[i32; 3]> {
    let k = keys();
    let block = block_at(host, level, pos)?;
    if block == k.block {
        return Some(pos);
    }
    let below = [pos[0], pos[1] - 1, pos[2]];
    (block == k.top && block_at(host, level, below) == Some(k.block)).then_some(below)
}

/// Whether there is room for a stone's upper block above `pos`.
pub(crate) fn room_above(level: &dyn LevelHost, pos: mod_api::BlockPos) -> bool {
    let Some(above) = level.block_state([pos.x, pos.y + 1, pos.z]) else {
        return true;
    };
    level.block_in_tag(above.block(), "minecraft:replaceable").unwrap_or(true)
}

/// The dimension's id, as the rules keep it.
fn dimension_id(dimension: LevelDimension) -> String {
    dimension.key().to_string()
}

/// A block as a player landing there sees it; none while it isn't loaded.
fn cell(host: &dyn ServerHost, level: LevelRef, pos: [i32; 3]) -> Option<rules::Cell> {
    if !level.is_loaded(host, pos) {
        return None;
    }
    let state = level.block_state(host, pos)?;
    Some(match state.block().id(host).as_str() {
        "minecraft:lava" | "minecraft:fire" | "minecraft:soul_fire" => rules::Cell::Hazard,
        _ if host.block_in_tag(state.block(), "minecraft:replaceable").unwrap_or(false) => rules::Cell::Passable,
        _ => rules::Cell::Solid,
    })
}

// ---- hook points ---------------------------------------------------------------

/// Sends the player the stones they know.
fn send_list(
    host: &mut dyn ServerHost,
    st: &State,
    p: &Pl,
    here: Option<u64>,
    open: bool,
    notice: Option<rules::Msg>,
) -> Result<(), String> {
    let entries = rules::entries(&st.reg, &actor(p), &p.dimension, p.position, here);
    let data = rules::encode_list(rules::ListMessage {
        open,
        entries,
        total: 0,
        notice,
        scroll: st.reg.needs_scroll(here),
        cooldown_ms: st.cooldowns.get(&p.uuid).map_or(0, |until| (until - now_ms()).max(0)),
    });
    p.player
        .send_payload(host, &PayloadId(rules::CHANNEL_LIST.into()), PayloadBytes(data))
        .map(drop)
        .map_err(|e| format!("{e:?}"))
}

/// Queues a travel, after the config's warmup.
fn queue_travel(host: &mut dyn ServerHost, st: &mut State, p: &Pl, to: u64, scroll: bool) {
    let Some(w) = st.reg.get(to) else {
        tell(host, p.player, red(rules::msg("wayfarer_stones.travel.gone", &[])));
        return;
    };
    let left = st.cooldowns.get(&p.uuid).map_or(0, |until| rules::seconds_left(*until, now_ms()));
    if left > 0 {
        tell(host, p.player, red(rules::msg("wayfarer_stones.travel.cooldown", &[&left])));
        return;
    }
    let cost = st.reg.cost((p.dimension.as_str(), p.position), w);
    let name = w.name.clone();
    st.pending.retain(|t| t.uuid != p.uuid);
    let warmup = i64::from(st.reg.config.warmup_seconds) * 1000;
    st.pending.push(rules::Pending {
        uuid: p.uuid,
        name: p.name.clone(),
        to,
        cost,
        scroll,
        from: (p.dimension.clone(), p.position),
        due: now_ms() + warmup,
    });
    if warmup > 0 {
        let seconds = st.reg.config.warmup_seconds;
        tell(host, p.player, gold(rules::msg("wayfarer_stones.travel.warmup", &[&name, &seconds])));
    }
}

/// A right click with the stone's item on a block, before the placement: false for a stone past the
/// owner's limit.
pub(crate) fn may_place(host: &mut dyn ServerHost, player: u128) -> bool {
    let Some(who) = by_uuid(host, player) else {
        return true;
    };
    let Some(mut guard) = world_state(host) else {
        return true;
    };
    let st = guard.as_mut().expect("opened");
    match st.reg.may_place(&actor(&who)) {
        Ok(()) => true,
        Err(e) => {
            drop(guard);
            let _ = who.player.action_bar(host, red(e));
            false
        }
    }
}

/// A player placed the stone: it registers itself, and its upper block follows at the tick's end.
pub(crate) fn placed(host: &mut dyn ServerHost, player: PlayerRef, pos: [i32; 3]) {
    let Some(who) = pl(host, player) else {
        return;
    };
    let Some(mut guard) = world_state(host) else {
        return;
    };
    let st = guard.as_mut().expect("opened");
    let w = st.reg.place(&who.dimension, pos, &actor(&who), now_ms());
    st.commands.push(rules::upper_block_command(&who.dimension, pos));
    drop(guard);
    tell(host, player, green(rules::msg("wayfarer_stones.placed", &[&w.name])));
}

/// A block changed, whoever changed it (a player, `/fill`, `/setblock`): a stone doesn't outlive its
/// lower block, and its two blocks go together. Runs for every change in the world, so it asks the
/// block first.
pub(crate) fn block_changed(level: &dyn LevelHost, event: &BlockChangedEvent) {
    let k = keys();
    let was = event.old.block();
    if (was != k.block && was != k.top) || event.new.block() == was {
        return;
    }
    let dimension = dimension_id(event.dimension);
    let pos = event.pos;
    let named = |y: i32| level.block_state([pos.x, y, pos.z]).map(|s| s.block());
    let mut state = lock();
    let Some(st) = state.as_mut() else {
        return;
    };
    if was == k.block {
        let at = [pos.x, pos.y, pos.z];
        if let Some(id) = st.reg.at(&dimension, at).map(|w| w.id) {
            st.reg.remove(id);
            st.pending.retain(|t| t.to != id);
            st.settling.retain(|t| t.to != id);
        }
        if named(pos.y + 1) == Some(k.top) {
            st.commands.push(rules::remove_block_command(&dimension, [pos.x, pos.y + 1, pos.z]));
        }
    } else if named(pos.y - 1) == Some(k.block) {
        st.commands.push(rules::remove_block_command(&dimension, [pos.x, pos.y - 1, pos.z]));
    }
}

/// A player breaks a block: a stone goes, or stays for its owner. True when it stays.
pub(crate) fn destroyed(host: &mut dyn ServerHost, pos: [i32; 3]) -> bool {
    let Some(who) = host.players().acting_player().and_then(|p| pl(host, p)) else {
        return false;
    };
    let Some(level) = who.player.level(host) else {
        return false;
    };
    let Some(lower) = lower_of(host, level, pos) else {
        return false;
    };
    let Some(mut guard) = world_state(host) else {
        return false;
    };
    let st = guard.as_mut().expect("opened");
    match st.reg.broken(&who.dimension, lower, &actor(&who)) {
        // `block_changed` takes the other block.
        Ok(None) => false,
        Ok(Some(w)) => {
            st.pending.retain(|t| t.to != w.id);
            drop(guard);
            tell(host, who.player, gold(rules::msg("wayfarer_stones.removed", &[&w.name])));
            false
        }
        Err(e) => {
            drop(guard);
            let _ = who.player.action_bar(host, red(e));
            true
        }
    }
}

/// A right click on a block: a stone activates and opens the screen. True when it was a stone. One a
/// village generated joins the registry here; a one-block stone from before 0.4 grows its upper block.
pub(crate) fn right_click(host: &mut dyn ServerHost, player: u128, pos: [i32; 3]) -> bool {
    let Some(who) = by_uuid(host, player) else {
        return false;
    };
    let Some(level) = who.player.level(host) else {
        return false;
    };
    let Some(lower) = lower_of(host, level, pos) else {
        return false;
    };
    let [x, y, z] = lower;
    let above = block_at(host, level, [x, y + 1, z]);
    let air = above.is_some_and(|b| b.id(host) == "minecraft:air");
    let Some(mut guard) = world_state(host) else {
        return true;
    };
    let st = guard.as_mut().expect("opened");
    let w = match st.reg.at(&who.dimension, lower).cloned() {
        Some(w) => w,
        None => st.reg.discover(&who.dimension, lower, now_ms()),
    };
    if above != Some(keys().top) && air {
        st.commands.push(rules::upper_block_command(&who.dimension, lower));
    }
    let activated = st.reg.activate(&uuid_string(who.uuid), w.id);
    if activated {
        st.commands.extend(rules::activation_commands(&w));
    }
    st.opens.request(rules::OpenRequest {
        uuid: who.uuid,
        here: Some(w.id),
    });
    drop(guard);
    if activated {
        tell(host, who.player, green(rules::msg("wayfarer_stones.activated", &[&w.name])));
    }
    true
}

/// A player uses a wayfarer's scroll: it opens the screen. True when it did.
pub(crate) fn use_scroll(host: &mut dyn ServerHost, player: u128) -> bool {
    let Some(mut guard) = world_state(host) else {
        return false;
    };
    let st = guard.as_mut().expect("opened");
    if !st.reg.config.wayfarers_scroll {
        return false;
    }
    st.opens.request(rules::OpenRequest {
        uuid: player,
        here: None,
    });
    true
}

/// The command line as the rules read it: its words, then its text argument.
fn command_line(event: &RunModCommandEvent<'_>) -> String {
    let mut line = event.command.literals.join(" ");
    if let Some(CommandArgumentValue::String(name)) = event.command.argument("name") {
        line.push(' ');
        line.push_str(&name);
    }
    line
}

/// A player's command: ours, when it is one.
pub(crate) fn command(host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
    let word = event.command.literals.first().map(String::as_str).unwrap_or_default();
    if !rules::COMMANDS.contains(&word) {
        return Flow::Continue;
    }
    let CommandSource::Player { player, .. } = event.source else {
        let _ = host.commands().feedback(red(rules::msg("wayfarer_stones.error.no_world", &[])));
        return Flow::Cancel;
    };
    let Some(p) = pl(host, *player) else {
        return Flow::Cancel;
    };
    let line = command_line(event);
    let Some(mut guard) = world_state(host) else {
        let _ = host.commands().feedback(red(rules::msg("wayfarer_stones.error.no_world", &[])));
        return Flow::Cancel;
    };
    let st = guard.as_mut().expect("opened");
    let mut replies = Vec::new();
    if let Some(e) = &st.broken {
        replies.push(red(rules::msg("wayfarer_stones.error.read_only", &[e])));
    }
    for o in st.reg.command(&actor(&p), &p.dimension, p.position, &line) {
        match o {
            rules::Out::Reply(s) => replies.push(green(s)),
            rules::Out::Error(s) => replies.push(red(s)),
            rules::Out::Open => st.opens.request(rules::OpenRequest {
                uuid: p.uuid,
                here: None,
            }),
            rules::Out::Travel { id, scroll } => queue_travel(host, st, &p, id, scroll),
        }
    }
    drop(guard);
    for reply in replies {
        let _ = host.commands().feedback(reply);
    }
    Flow::Cancel
}

/// A request from a player's client, checked here as any player's input.
struct FromClient;

impl IPayloadHandler for FromClient {
    fn handle(&self, payload: PayloadBytes, context: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let player = context.player().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        let host = context.server_host().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        let Some(act) = rules::decode_act(&payload.0) else {
            return Err(PayloadHandleError::Failed("an unreadable request from a client".into()));
        };
        from_client(host, player, act);
        Ok(())
    }
}

fn from_client(host: &mut dyn ServerHost, player: PlayerRef, act: rules::ClientAct) {
    let Some(p) = pl(host, player) else {
        return;
    };
    let Some(mut guard) = world_state(host) else {
        return;
    };
    let st = guard.as_mut().expect("opened");
    let who = actor(&p);
    match act {
        rules::ClientAct::Travel(id) => {
            // Only from a screen this server opened, once.
            let Some(session) = st.sessions.remove(&p.uuid) else {
                return;
            };
            if session.scroll && !st.reg.config.wayfarers_scroll {
                drop(guard);
                tell(host, player, red(rules::msg("wayfarer_stones.error.stand_at_stone", &[])));
            } else if st.reg.knows(&who.uuid, id) && session.here != Some(id) {
                queue_travel(host, st, &p, id, session.scroll);
            } else {
                drop(guard);
                tell(host, player, red(rules::msg("wayfarer_stones.travel.refused", &[])));
            }
        }
        rules::ClientAct::Rename { id, name } => {
            let notice = st.reg.rename(id, &name, &who).err();
            let here = st.sessions.get(&p.uuid).and_then(|s| s.here);
            if let Err(e) = send_list(host, st, &p, here, false, notice) {
                log::warn!("[wayfarer_stones] {e}");
            }
        }
    }
}

// ---- the server's tick ---------------------------------------------------------

/// A due travel, as console commands to run with the state's lock let go.
struct Travel {
    player: PlayerRef,
    uuid: u128,
    /// Fails when the player lacks the levels; none for a free travel.
    check: Option<String>,
    /// Takes the wayfarer's scroll; fails when the player has none.
    scroll: Option<String>,
    refund: Option<String>,
    /// What the player leaves behind.
    departure: [String; 2],
    /// Where they go, as far as can be told now.
    landing: rules::Landing,
    /// The stone's top, where they wait when nobody can tell yet.
    top: [f64; 3],
    dimension: String,
    settling: rules::Settling,
    cost: u32,
    name: String,
}

fn run(host: &mut dyn ServerHost, command: &str) -> bool {
    host.commands().run_command(command)
}

fn give_levels(host: &mut dyn ServerHost, uuid: u128, levels: i32) {
    if let Some(player) = host.player(uuid) {
        player.give_levels(host, levels);
    }
}

fn tell_uuid(host: &mut dyn ServerHost, uuid: u128, line: Text) {
    if let Some(player) = host.player(uuid) {
        tell(host, player, line);
    }
}

/// The end of each server tick: due travels are made, and the files that changed are written once a
/// second.
pub(crate) fn tick(host: &mut dyn ServerHost) {
    let n = TICKS.fetch_add(1, Relaxed);
    let Some(mut guard) = world_state(host) else {
        return;
    };
    let st = guard.as_mut().expect("opened");
    let second = n % 20 == 0;
    let idle = st.pending.is_empty() && st.opens.is_empty() && st.settling.is_empty();
    if idle && st.commands.is_empty() && !second {
        return;
    }
    let now = now_ms();
    st.cooldowns.retain(|_, until| *until > now);
    let players: Vec<Pl> = if idle {
        Vec::new()
    } else {
        host.players().server_players().into_iter().filter_map(|p| pl(host, p)).collect()
    };
    // Screens: here, where the server's connections are back in place.
    for r in st.opens.take(n) {
        let Some(p) = players.iter().find(|p| p.uuid == r.uuid) else {
            continue;
        };
        let here = r
            .here
            .or_else(|| rules::here_of(&st.reg, &uuid_string(p.uuid), &p.dimension, p.position));
        let scroll = st.reg.needs_scroll(here);
        st.sessions.insert(p.uuid, rules::Session { scroll, here });
        if let Err(e) = send_list(host, st, p, here, true, None) {
            log::warn!("[wayfarer_stones] {e}");
            tell(host, p.player, red(rules::msg("wayfarer_stones.screen.cant_open", &[])));
        }
    }
    // Players waiting on a stone for its chunks: on to the safe place, or back.
    let mut moves: Vec<String> = Vec::new();
    let mut undo: Vec<rules::Settling> = Vec::new();
    let levels: HashMap<String, LevelRef> = host
        .levels()
        .list_levels()
        .into_iter()
        .filter_map(|level| Some((level.dimension().id(host)?, level)))
        .collect();
    let landing_spot = |w: &rules::Stone| match levels.get(&w.dimension) {
        Some(level) => rules::landing_spot(w, &|pos| cell(host, *level, pos)),
        None => rules::Landing::Unknown,
    };
    st.settling.retain(|t| {
        let Some(w) = st.reg.get(t.to) else {
            return false;
        };
        let top = rules::landing(w);
        let Some(p) = players.iter().find(|p| p.uuid == t.uuid) else {
            return false; // left
        };
        // Gone on by themselves: it is no business of ours any more.
        if p.dimension != w.dimension || rules::distance(p.position, top) > 3.0 {
            return false;
        }
        match landing_spot(w) {
            rules::Landing::Unknown => n < t.until,
            rules::Landing::At(spot) => {
                if spot != top {
                    moves.push(rules::travel_command(&t.name, &w.dimension, spot));
                }
                false
            }
            rules::Landing::Blocked => {
                undo.push(t.clone());
                false
            }
        }
    });
    // Travels: each is resolved to the commands to run, then the lock is let go before any command
    // runs (a teleport may fire other hooks).
    let mut go: Vec<Travel> = Vec::new();
    let mut told: Vec<(PlayerRef, Text)> = Vec::new();
    st.pending.retain(|t| {
        let Some(p) = players.iter().find(|p| p.uuid == t.uuid) else {
            return false; // left
        };
        if rules::moved(&t.from, (p.dimension.as_str(), p.position)) {
            told.push((p.player, red(rules::msg("wayfarer_stones.travel.moved", &[]))));
            return false;
        }
        if t.due > now {
            return true;
        }
        let Some(w) = st.reg.get(t.to) else {
            told.push((p.player, red(rules::msg("wayfarer_stones.travel.gone", &[]))));
            return false;
        };
        go.push(Travel {
            player: p.player,
            uuid: t.uuid,
            check: (t.cost > 0).then(|| rules::level_check_command(&t.name, t.cost)),
            scroll: t.scroll.then(|| rules::consume_scroll_command(&t.name)),
            refund: t.scroll.then(|| rules::refund_scroll_command(&t.name)),
            departure: rules::departure_commands(&p.dimension, p.position),
            landing: landing_spot(w),
            top: rules::landing(w),
            dimension: w.dimension.clone(),
            settling: rules::Settling {
                uuid: t.uuid,
                name: t.name.clone(),
                to: t.to,
                from: (p.dimension.clone(), p.position),
                cost: t.cost,
                scroll: t.scroll,
                until: n + rules::SETTLE_TICKS,
            },
            cost: t.cost,
            name: w.name.clone(),
        });
        false
    });
    if second {
        save(st);
    }
    let commands = std::mem::take(&mut st.commands);
    drop(guard);
    for (player, line) in told {
        tell(host, player, line);
    }
    for command in commands.iter().chain(&moves) {
        if !run(host, command) {
            log::warn!("[wayfarer_stones] failed: {command}");
        }
    }
    for t in undo {
        run(host, &rules::travel_command(&t.name, &t.from.0, t.from.1));
        if t.scroll {
            run(host, &rules::refund_scroll_command(&t.name));
        }
        if t.cost > 0 {
            give_levels(host, t.uuid, i32::try_from(t.cost).unwrap_or(i32::MAX));
        }
        tell_uuid(host, t.uuid, red(rules::msg("wayfarer_stones.travel.blocked", &[])));
    }
    let mut travelled: Vec<u128> = Vec::new();
    let mut waits: Vec<rules::Settling> = Vec::new();
    for t in go {
        // In this order: nothing is charged for a travel that didn't happen.
        let (spot, wait) = match t.landing {
            rules::Landing::At(spot) => (spot, false),
            rules::Landing::Unknown => (t.top, true),
            rules::Landing::Blocked => {
                tell(host, t.player, red(rules::msg("wayfarer_stones.travel.blocked", &[])));
                continue;
            }
        };
        if let Some(check) = &t.check
            && !run(host, check)
        {
            let why = rules::msg("wayfarer_stones.travel.levels", &[&t.cost, &t.name]);
            tell(host, t.player, red(why));
            continue;
        }
        if let Some(scroll) = &t.scroll
            && !run(host, scroll)
        {
            tell(host, t.player, red(rules::msg("wayfarer_stones.travel.no_scroll", &[])));
            continue;
        }
        // Before the player goes: `tp` moves them out of earshot of it.
        for command in &t.departure {
            run(host, command);
        }
        if !run(host, &rules::travel_command(&t.settling.name, &t.dimension, spot)) {
            if let Some(refund) = &t.refund {
                run(host, refund);
            }
            tell(host, t.player, red(rules::msg("wayfarer_stones.travel.failed", &[&t.name])));
            continue;
        }
        for command in &rules::arrival_commands(&t.dimension, spot) {
            run(host, command);
        }
        travelled.push(t.uuid);
        if wait {
            waits.push(t.settling);
        }
        if t.cost > 0 {
            give_levels(host, t.uuid, -i32::try_from(t.cost).unwrap_or(i32::MAX));
        }
        tell(host, t.player, green(rules::msg("wayfarer_stones.travel.done", &[&t.name])));
    }
    if travelled.is_empty() {
        return;
    }
    let mut state = lock();
    if let Some(st) = state.as_mut() {
        let until = now_ms() + i64::from(st.reg.config.cooldown_seconds) * 1000;
        if st.reg.config.cooldown_seconds > 0 {
            st.cooldowns.extend(travelled.iter().map(|uuid| (*uuid, until)));
        }
        st.settling.extend(waits);
    }
}
