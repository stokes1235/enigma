#![cfg(windows)]
mod win;

use std::{collections::{HashMap, VecDeque}, env, ffi::OsString, fs::{self, File, OpenOptions}, io::{self, Read, Write}, os::windows::process::CommandExt, path::{Component, Path, PathBuf}, process::{Child, Command, Stdio}, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, OnceLock}, thread, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

static LAUNCH_LOG: OnceLock<Mutex<File>> = OnceLock::new();
fn emit(error: bool, line: String) {
    if error { let _=writeln!(io::stderr(), "{line}"); } else { let _=writeln!(io::stdout(), "{line}"); }
    if let Some(log)=LAUNCH_LOG.get() { if let Ok(mut file)=log.lock() {
        let timestamp=win::local_timestamp();
        let _=writeln!(file, "[{timestamp}] [{}] {line}", if error {"ERROR"} else {"INFO"});
        let _=file.flush();
    }}
}
macro_rules! say { ($($arg:tt)*) => { emit(false,format!($($arg)*)) }; }
macro_rules! warn { ($($arg:tt)*) => { emit(true,format!($($arg)*)) }; }
mod guardian;

const CONFIG_NAME: &str = "r1999-launcher.ini";
const INDEPENDENT_PROCESS: u32 = 0x0800_0000 | 0x0000_0200; // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP

#[derive(Default)]
struct PathConfig { root:Option<PathBuf>, game:Option<PathBuf>, logs:Option<PathBuf> }
impl PathConfig {
    fn parse(text:&str,dir:&Path)->Result<Self,String> {
        let mut result=Self::default();
        for (index,raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
            let line=raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {continue;}
            if line.starts_with('[') {
                if !line.ends_with(']') || !line[1..line.len()-1].trim().eq_ignore_ascii_case("paths") {
                    return Err(format!("第 {} 行：只支持 [paths] 区域",index+1));
                }
                continue;
            }
            let (name,value)=line.split_once('=').ok_or_else(||format!("第 {} 行：应写成 名称=路径",index+1))?;
            let name=name.trim().to_ascii_lowercase();
            let value=value.trim();
            let value=if value.starts_with('"') || value.ends_with('"') {
                if value.len()<2 || !value.starts_with('"') || !value.ends_with('"') {return Err(format!("第 {} 行：路径引号不完整",index+1));}
                &value[1..value.len()-1]
            } else {value};
            if value.is_empty() || value.contains('\0') {return Err(format!("第 {} 行：路径不能为空",index+1));}
            let path=PathBuf::from(value);
            let path=if path.is_absolute() {path} else {
                if path.has_root() || matches!(path.components().next(),Some(Component::Prefix(_))) {
                    return Err(format!("第 {} 行：请使用完整的绝对路径或普通相对路径",index+1));
                }
                dir.join(path)
            };
            let slot=match name.as_str() {
                "server_root"=>&mut result.root,
                "game_dir"=>&mut result.game,
                "log_dir"=>&mut result.logs,
                _=>return Err(format!("第 {} 行：不认识的配置项 {name}",index+1)),
            };
            if slot.is_some(){return Err(format!("第 {} 行：配置项 {name} 重复",index+1));}
            *slot=Some(path);
        }
        Ok(result)
    }
}

struct Options { root:PathBuf, game:PathBuf, logs:PathBuf, config:Option<PathBuf>, pause:bool, game_timeout:u64 }
impl Options {
    fn parse_from(dir:&Path,mut args:impl Iterator<Item=OsString>)->Result<Option<Self>,String> {
        let mut overrides=PathConfig::default();
        let mut pause=true;
        while let Some(arg)=args.next() {
            match arg.to_string_lossy().as_ref() {
                "--root"=>overrides.root=Some(args.next().ok_or("--root 缺少路径")?.into()),
                "--game-dir"=>overrides.game=Some(args.next().ok_or("--game-dir 缺少路径")?.into()),
                "--log-dir"=>overrides.logs=Some(args.next().ok_or("--log-dir 缺少路径")?.into()),
                "--no-pause"=>pause=false,
                "--help"|"-h"=>{say!("R1999 启动器\n路径配置: 启动器同目录的 {CONFIG_NAME}\n可选参数: --root <服务端根目录> --game-dir <游戏目录> --log-dir <日志目录> --no-pause");return Ok(None);},
                x=>return Err(format!("不认识的参数: {x}")),
            }
        }
        let config_path=dir.join(CONFIG_NAME);
        let (configured,config)=match fs::read_to_string(&config_path) {
            Ok(text)=>(PathConfig::parse(&text,dir).map_err(|e|format!("路径配置 {} 无效：{e}",config_path.display()))?,Some(config_path.clone())),
            Err(e) if e.kind()==io::ErrorKind::NotFound=>(PathConfig::default(),None),
            Err(e) if e.kind()==io::ErrorKind::InvalidData=>return Err(format!("路径配置 {} 不是 UTF-8，请用记事本另存为 UTF-8",config_path.display())),
            Err(e)=>return Err(format!("读取路径配置 {} 失败: {e}",config_path.display())),
        };
        let root=overrides.root.or(configured.root).unwrap_or_else(||dir.to_owned());
        let game=overrides.game.or(configured.game).ok_or_else(||format!("未设置游戏路径。请在启动器同目录的 {} 中设置 game_dir，或使用 --game-dir。",config_path.display()))?;
        let logs=overrides.logs.or(configured.logs).unwrap_or_else(||dir.join("logs"));
        Ok(Some(Self{root,game,logs,config,pause,game_timeout:20}))
    }
    fn parse()->Result<Option<Self>,String> {
        let exe=env::current_exe().map_err(|e|format!("无法确定启动器位置: {e}"))?;
        let dir=exe.parent().ok_or("无法确定启动器所在目录")?;
        Self::parse_from(dir,env::args_os().skip(1))
    }
}

#[derive(Debug)]
struct Ports { game: u16, sdk: u16, muip: u16, gm: Option<u16> }
fn read_ports(path: &Path) -> Result<Ports, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    let mut section = String::new();
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') { section = line[1..line.len()-1].trim().to_owned(); continue; }
        if let Some((k,v)) = line.split_once('=') { values.insert((section.clone(),k.trim().to_owned()), v.trim().trim_matches('"').to_owned()); }
    }
    let port = |section: &str, key: &str| -> Result<u16,String> {
        let value = values.get(&(section.to_owned(),key.to_owned())).ok_or_else(|| format!("配置缺少 [{section}] {key}"))?;
        let n = value.parse::<u16>().map_err(|_| format!("端口不是有效数字: [{section}] {key}={value}"))?;
        if n == 0 { Err(format!("端口不能为零: [{section}] {key}")) } else { Ok(n) }
    };
    let enabled = values.get(&("muip_gm".into(),"enabled".into())).map(String::as_str).unwrap_or("false");
    let gm = match enabled { "true" => Some(port("muip_gm","port")?), "false" => None, _ => return Err("[muip_gm] enabled 应为 true 或 false".into()) };
    let p = Ports { game: port("server","game_port")?, sdk: port("server","http_port")?, muip: port("muip","port")?, gm };
    let mut all = vec![p.game,p.sdk,p.muip]; if let Some(n)=p.gm { all.push(n); }
    all.sort_unstable(); all.dedup();
    if all.len() != 3 + usize::from(p.gm.is_some()) { return Err("配置中的服务端口重复".into()); }
    Ok(p)
}
fn ports_list(p:&Ports) -> Vec<u16> { let mut v=vec![p.game,p.sdk,p.muip]; if let Some(n)=p.gm {v.push(n)} v }
fn ensure_ports_free(ports:&[u16]) -> Result<(),String> {
    for port in ports {
        // A port can have multiple listeners, so keep checking until it is free.
        for _ in 0..32 {
            let listeners=win::listeners().map_err(|e|format!("检查端口失败: {e}"))?;
            let Some((_,pid))=listeners.into_iter().find(|(p,_)|p==port) else { break };
            if pid==0 || pid==std::process::id() {return Err(format!("端口 {port} 被系统或启动器自身占用 (PID {pid})，无法自动释放"));}
            let process=match win::WatchedProcess::open(pid,None,true) {
                Ok(p)=>p,
                Err(e)=>{
                    if !win::listeners().map_err(|e|format!("检查端口失败: {e}"))?.contains(&(*port,pid)) {continue;}
                    return Err(format!("无法打开端口 {port} 的占用进程 {pid}: {e}"));
                }
            };
            // Hold the process handle while checking the listener, preventing PID reuse
            // from redirecting termination to another process.
            if !process.is_running().map_err(|e|format!("检查进程 {pid} 失败: {e}"))? ||
                !win::listeners().map_err(|e|format!("检查端口失败: {e}"))?.contains(&(*port,pid)) {continue;}
            let name=win::processes().ok().and_then(|ps|ps.into_iter().find(|p|p.pid==pid).map(|p|p.name)).unwrap_or_else(||"未知程序".into());
            warn!("端口 {port} 被 {name} (PID {pid}) 占用，正在结束该进程");
            process.terminate().map_err(|e|format!("结束端口 {port} 的占用进程 {pid} 失败: {e}"))?;
            say!("已结束进程 {pid}，正在确认端口 {port} 已释放");
        }
        if let Some((_,pid))=win::listeners().map_err(|e|format!("检查端口失败: {e}"))?.into_iter().find(|(p,_)|p==port) {
            return Err(format!("端口 {port} 仍被进程 {pid} 占用，无法启动服务"));
        }
    }
    Ok(())
}
fn game_processes() -> Result<Vec<win::Process>,String> {
    win::processes().map(|v| v.into_iter().filter(|p|p.name.eq_ignore_ascii_case("reverse1999.exe")).collect()).map_err(|e|format!("读取游戏进程失败: {e}"))
}
fn must_file(path:&Path) -> Result<(),String> {
    if path.is_file() {Ok(())} else {Err(format!("找不到所需文件: {}",path.display()))}
}
fn preflight(o:&mut Options) -> Result<Ports,String> {
    o.root=fs::canonicalize(&o.root).map_err(|e|format!("服务端目录不可用 {}: {e}",o.root.display()))?;
    o.game=fs::canonicalize(&o.game).map_err(|e|format!("游戏目录不可用 {}: {e}",o.game.display()))?;
    for relative in ["Server/gameserver.exe","Server/sdkserver.exe","Server/muipserver.exe","Server/config.toml","Patch/launcher.exe","Patch/sonetto.dll"] {must_file(&o.root.join(relative))?;}
    must_file(&o.game.join("reverse1999.exe"))?;
    if o.game.join("launcher.exe").exists() {return Err(format!("游戏目录已有 launcher.exe，无法确认是否属于其他程序，不会覆盖或删除: {}",o.game.join("launcher.exe").display()));}
    let dll=o.game.join("sonetto.dll");
    if dll.exists() && !dll.is_file() {return Err(format!("游戏目录中的 sonetto.dll 不是文件: {}",dll.display()));}
    let existing=game_processes()?;
    if !existing.is_empty() {return Err(format!("已有游戏进程运行 (PID {:?})，请先关闭，避免影响现有游戏",existing.iter().map(|p|p.pid).collect::<Vec<_>>()));}
    let p=read_ports(&o.root.join("Server/config.toml"))?;
    ensure_ports_free(&[p.game,p.sdk,p.muip])?;
    if let Some(port)=p.gm {
        if let Some((_,pid))=win::listeners().map_err(|e|format!("检查端口失败: {e}"))?.into_iter().find(|(n,_)|*n==port) {
            return Err(format!("GM 端口 {port} 被进程 {pid} 占用，无法启动服务"));
        }
    }
    Ok(p)
}

struct OwnedFile { path:PathBuf, id:win::FileId, size:u64, modified:SystemTime }
impl OwnedFile {
    fn create(source:&Path,target:&Path) -> Result<Self,String> {
        let mut src=File::open(source).map_err(|e|format!("打开 {} 失败: {e}",source.display()))?;
        let mut dst=OpenOptions::new().write(true).create_new(true).open(target).map_err(|e|format!("创建 {} 失败（不会覆盖已有文件）: {e}",target.display()))?;
        if let Err(e)=io::copy(&mut src,&mut dst).and_then(|_|dst.flush()) {
            drop(dst); say!("准备删除本次未完成的文件: {}",target.display()); let _=fs::remove_file(target);
            return Err(format!("复制 {} 失败: {e}",target.display()));
        }
        let id=match win::file_id(&dst) {Ok(x)=>x,Err(e)=>{drop(dst); say!("准备删除本次未完成的文件: {}",target.display()); let _=fs::remove_file(target); return Err(format!("确认文件身份失败: {e}"));}};
        let meta=match dst.metadata() {Ok(m)=>m,Err(e)=>{drop(dst); say!("准备删除本次未完成的文件: {}",target.display()); let _=fs::remove_file(target); return Err(format!("读取文件信息失败: {e}"));}};
        let modified=match meta.modified() {Ok(t)=>t,Err(e)=>{drop(dst); say!("准备删除本次未完成的文件: {}",target.display()); let _=fs::remove_file(target); return Err(format!("读取文件修改时间失败: {e}"));}};
        Ok(Self{path:target.to_owned(),id,size:meta.len(),modified})
    }
    fn remove_if_unchanged(self) -> Result<(),String> {
        let result=(|| -> io::Result<bool> {
            let f=match File::open(&self.path) {Ok(f)=>f,Err(e) if e.kind()==io::ErrorKind::NotFound=>return Ok(true),Err(e)=>return Err(e)};
            let id=win::file_id(&f)?; let m=f.metadata()?;
            if id!=self.id || m.len()!=self.size || m.modified()?!=self.modified {return Ok(false)}
            drop(f);
            say!("准备删除本次复制的文件: {}",self.path.display());
            fs::remove_file(&self.path)?; Ok(true)
        })();
        match result {
            Ok(true)=>Ok(()),
            Ok(false)=>Err(format!("文件已变化，保留未删除: {}",self.path.display())),
            Err(e)=>Err(format!("无法删除文件 {}: {e}",self.path.display())),
        }
    }
}

// Only report identifiers actually present in a line emitted by the server.
fn identifiers(line:&str)->Vec<(&'static str,u64)> {
    let fields=[("skill_effect_id","技能效果 ID"),("skill_id","技能 ID"),("effect_id","效果 ID"),
        ("card_id","卡牌 ID"),("skillId","技能 ID"),("cardId","卡牌 ID")];
    let mut out=Vec::new();
    for (key,label) in fields {
        for (pos,_) in line.match_indices(key) {
            if pos>0 {let b=line.as_bytes()[pos-1];if b.is_ascii_alphanumeric() || b==b'_' {continue;}}
            let rest=line[pos+key.len()..].trim_start_matches(|c:char|c.is_ascii_whitespace() || c=='"');
            let rest=if let Some(v)=rest.strip_prefix(':').or_else(||rest.strip_prefix('=')){v}else{continue};
            let rest=rest.trim_start_matches(|c:char|c.is_ascii_whitespace() || c=='"');
            let rest=rest.strip_prefix("Some(").unwrap_or(rest);
            let digits:String=rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id)=digits.parse::<u64>() {if !out.contains(&(label,id)){out.push((label,id));}}
        }
    }
    out
}

fn is_error_line(line:&str)->bool {line.contains("ERROR") || line.contains("panicked")}
fn error_identifiers(line:&str)->Vec<(&'static str,u64)> {
    if is_error_line(line) {identifiers(line)} else {Vec::new()}
}

// Keep the game's own commands and identifier-bearing messages in their original order.
// A command identifies an operation, not necessarily the skill executed by the battle engine.
fn plain_service_line(line:&str)->String {
    let mut chars=line.chars().peekable();
    let mut out=String::new();
    while let Some(c)=chars.next() {
        if c=='\u{1b}' && chars.peek()==Some(&'[') {
            chars.next();
            for code in chars.by_ref() {if ('@'..='~').contains(&code) {break;}}
        } else if !c.is_control() || c=='\t' {out.push(c);}
    }
    out
}
fn received_command(line:&str)->Option<&str> {
    let rest=line.split_once("Received Cmd:")?.1.trim_start();
    let command=rest.split(|c:char|!c.is_ascii_alphanumeric() && c!='_').next()?;
    if command.is_empty() {None} else {Some(command)}
}
#[derive(Default)]
struct BattleTrace { recent:VecDeque<String>, last_entry:Option<String> }
impl BattleTrace {
    fn observe(&mut self,line:&str)->Option<String> {
        let line=plain_service_line(line);
        if is_error_line(&line) {return None;}
        let command=received_command(&line);
        let is_command=command.map(|c|c!="GetServerTimeCmd").unwrap_or(false);
        let has_id=!identifiers(&line).is_empty() || line.contains("buff_id=");
        if !is_command && !has_id {return None;}
        let mut short:String=line.chars().take(1600).collect();
        if line.chars().count()>1600 {short.push_str(" …（已截断，请查看原始游戏服务日志）");}
        if let Some(cmd)=command {
            if (cmd.starts_with("Start") || cmd.starts_with("Enter"))
                && (cmd.contains("Act") || cmd.contains("Battle") || cmd.contains("Dungeon") || cmd.contains("Fight")) {
                self.last_entry=Some(short.clone());
            }
        }
        if self.recent.len()==40 {self.recent.pop_front();}
        self.recent.push_back(short.clone());
        Some(short)
    }
    fn context(&self)->Vec<String> {
        let mut out=vec!["[错误前轨迹] 以下是服务端最近记录的命令和带编号的信息，按原文列出；警告中的编号不一定代表已出牌。".to_owned()];
        if let Some(entry)=&self.last_entry {out.push(format!("[错误前轨迹] 最近活动或战斗入口: {entry}"));}
        else {out.push("[错误前轨迹] 未找到活动或战斗入口记录。".to_owned());}
        if self.recent.is_empty(){out.push("[错误前轨迹] 没有更多操作记录。".to_owned());}
        else {for line in &self.recent {out.push(format!("[错误前轨迹] {line}"));}}
        out.push("[错误前轨迹] 若日志未明确记录出牌技能或卡牌 ID，则具体出牌未知。".to_owned());
        out
    }
}

// Read the independent service log files instead of connecting child stdout to
// the launcher process. Closing the console therefore cannot break a service pipe.
struct LogTail { stop:Arc<AtomicBool>, worker:Option<thread::JoinHandle<()>> }
fn drain_lines(pending:&mut Vec<u8>, flush:bool, output:&mut dyn FnMut(String)) {
    while let Some(end)=pending.iter().position(|&b|b==b'\n') {
        let mut bytes:Vec<u8>=pending.drain(..=end).collect();
        bytes.pop();
        if bytes.last()==Some(&b'\r') {bytes.pop();}
        output(String::from_utf8_lossy(&bytes).into_owned());
    }
    // Bound memory if a service writes a very long line without a newline.
    while pending.len()>65_536 {
        let mut end=65_536;
        while end>0 && pending[end]&0xc0==0x80 {end-=1;}
        if end==0 {end=65_536;}
        let bytes:Vec<u8>=pending.drain(..end).collect();
        output(String::from_utf8_lossy(&bytes).into_owned());
    }
    if flush && !pending.is_empty() {
        output(String::from_utf8_lossy(pending).into_owned());
        pending.clear();
    }
}
impl LogTail {
    fn start(mut file:File, mut output:impl FnMut(String)+Send+'static)->io::Result<Self> {
        let stop=Arc::new(AtomicBool::new(false));
        let should_stop=Arc::clone(&stop);
        let worker=thread::Builder::new().name("service-log-display".into()).spawn(move||{
            let mut input=[0u8;8192];
            let mut pending=Vec::new();
            loop {
                match file.read(&mut input) {
                    Ok(0)=>{
                        if should_stop.load(Ordering::Acquire){break;}
                        thread::sleep(Duration::from_millis(100));
                    },
                    Ok(n)=>{pending.extend_from_slice(&input[..n]);drain_lines(&mut pending,false,&mut output);},
                    Err(e) if e.kind()==io::ErrorKind::Interrupted=>continue,
                    Err(e)=>{output(format!("无法读取服务日志: {e}"));break;},
                }
            }
            drain_lines(&mut pending,true,&mut output);
        })?;
        Ok(Self{stop,worker:Some(worker)})
    }
    fn finish(mut self) {
        self.stop.store(true,Ordering::Release);
        if let Some(worker)=self.worker.take(){let _=worker.join();}
    }
}

struct Managed { name:&'static str, child:Child, reported:bool }
struct Runtime { servers:Vec<Managed>, files:Vec<OwnedFile>, helper:Option<Child>, tails:Vec<LogTail>, log_dir:PathBuf }
impl Runtime {
    fn new(base:&Path)->Result<Self,String> {
        fs::create_dir_all(base).map_err(|e|format!("创建日志目录 {} 失败: {e}",base.display()))?;
        let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let dir=base.join(format!("run-{now}-{}",std::process::id()));
        fs::create_dir(&dir).map_err(|e|format!("创建本次日志目录失败: {e}"))?;
        let activity=OpenOptions::new().create_new(true).write(true).open(dir.join("startup.log")).map_err(|e|format!("创建启动器日志失败: {e}"))?;
        LAUNCH_LOG.set(Mutex::new(activity)).map_err(|_|"启动器日志已初始化")?;
        say!("本次日志目录: {}",dir.display());
        Ok(Self{servers:Vec::new(),files:Vec::new(),helper:None,tails:Vec::new(),log_dir:dir})
    }
    fn spawn(&mut self, name:&'static str, exe:&Path, cwd:&Path)->Result<(),String> {
        let logfile=self.log_dir.join(format!("{name}.log"));
        let log=OpenOptions::new().create_new(true).write(true).open(&logfile).map_err(|e|format!("创建日志失败: {e}"))?;
        let err=log.try_clone().map_err(|e|format!("打开日志失败: {e}"))?;
        let reader=if name=="gameserver" || name=="sdkserver" {
            Some(File::open(&logfile).map_err(|e|format!("打开日志以供显示失败: {e}"))?)
        } else {None};
        let mut trace_file=if name=="gameserver" {
            let path=self.log_dir.join("battle-context.log");
            let f=OpenOptions::new().create_new(true).write(true).open(&path).map_err(|e|format!("创建战斗轨迹日志失败: {e}"))?;
            say!("战斗操作轨迹: {}",path.display());
            Some(f)
        } else {None};
        let child=Command::new(exe).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::from(log)).stderr(Stdio::from(err)).creation_flags(INDEPENDENT_PROCESS).spawn().map_err(|e|format!("启动 {} 失败: {e}",exe.display()))?;
        say!("已启动 {name}（PID {}），日志: {}",child.id(),logfile.display());
        self.servers.push(Managed{name,child,reported:false});
        if let Some(reader)=reader {
            let mut trace=BattleTrace::default();
            let tail=LogTail::start(reader,move|line|{
                // The servers write terminal color escapes to their log files.
                // The Windows console may show those escapes literally, so clean
                // the display line before both printing and extracting error IDs.
                let line=plain_service_line(&line);
                let mut stdout=io::stdout().lock();
                let _=writeln!(stdout,"[{name}] {line}");
                if name=="gameserver" {
                    let mut records=Vec::new();
                    if is_error_line(&line) {
                        records.push(format!("[游戏服务错误] {line}"));
                        let ids=error_identifiers(&line);
                        if !ids.is_empty() {
                            let details=ids.iter().map(|(label,id)|format!("{label}: {id}")).collect::<Vec<_>>().join("；");
                            let entry=format!("[战斗错误定位] {details}");
                            let _=writeln!(stdout,"{entry}");
                            records.push(entry);
                        }
                        for entry in trace.context() {let _=writeln!(stdout,"{entry}");records.push(entry);}
                    } else if let Some(entry)=trace.observe(&line) {records.push(entry);}
                    if let Some(file)=trace_file.as_mut() {
                        let saved=(||->io::Result<()> {
                            for entry in &records {writeln!(file,"{entry}")?;}
                            if !records.is_empty(){file.flush()?;}
                            Ok(())
                        })();
                        if let Err(e)=saved {
                            let _=writeln!(stdout,"[战斗轨迹] 保存失败: {e}；原始日志仍保存在 gameserver.log");
                            trace_file=None;
                        }
                    }
                }
                let _=stdout.flush();
            }).map_err(|e|format!("启动 {name} 实时日志显示失败: {e}"))?;
            self.tails.push(tail);
        }
        Ok(())
    }
    fn wait_ready(&mut self, name:&str, port:u16)->Result<(),String> {
        let s=self.servers.iter_mut().find(|s|s.name==name).ok_or("内部服务状态错误")?;
        let deadline=Instant::now()+Duration::from_secs(90);
        loop {
            if let Some(status)=s.child.try_wait().map_err(|e|format!("读取 {name} 状态失败: {e}"))? {return Err(format!("{name} 启动后退出，状态 {status}，请查看 {}",self.log_dir.join(format!("{name}.log")).display()));}
            if let Some((_,pid))=win::listeners().map_err(|e|format!("检查端口失败: {e}"))?.into_iter().find(|(p,_)|*p==port) {
                if pid==s.child.id() {say!("{name} 已监听端口 {port}"); return Ok(())}
                return Err(format!("端口 {port} 被其他进程 {pid} 占用；未关闭该进程"));
            }
            if Instant::now()>deadline {return Err(format!("等待 {name} 的端口 {port} 超时，请查看服务日志"));}
            thread::sleep(Duration::from_millis(200));
        }
    }
    fn copy_patches(&mut self,root:&Path,game:&Path)->Result<(),String> {
        let dll=game.join("sonetto.dll");
        if dll.exists() {say!("沿用游戏目录中已有的 sonetto.dll，不覆盖、不删除: {}",dll.display());}
        else {let f=OwnedFile::create(&root.join("Patch/sonetto.dll"),&dll)?; say!("已复制补丁: {}",dll.display()); self.files.push(f);}
        let target=game.join("launcher.exe");
        let f=OwnedFile::create(&root.join("Patch/launcher.exe"),&target)?; say!("已复制补丁: {}",target.display()); self.files.push(f);
        Ok(())
    }
    fn spawn_helper(&mut self,game:&Path)->Result<u32,String> {
        let logpath=self.log_dir.join("launcher.log");
        let log=OpenOptions::new().create_new(true).write(true).open(&logpath).map_err(|e|format!("创建补丁日志失败: {e}"))?;
        let err=log.try_clone().map_err(|e|format!("打开补丁日志失败: {e}"))?;
        let child=Command::new(game.join("launcher.exe")).current_dir(game).stdin(Stdio::null()).stdout(Stdio::from(log)).stderr(Stdio::from(err)).creation_flags(INDEPENDENT_PROCESS).spawn().map_err(|e|format!("启动游戏补丁失败: {e}"))?;
        let pid=child.id(); self.helper=Some(child); say!("已启动游戏补丁（PID {pid}）"); Ok(pid)
    }
    fn helper_exited(&mut self)->Result<Option<std::process::ExitStatus>,String> {
        self.helper.as_mut().ok_or("补丁进程状态缺失")?.try_wait().map_err(|e|format!("读取补丁进程状态失败: {e}"))
    }
    fn report_dead_services(&mut self) {
        for s in &mut self.servers { if !s.reported { match s.child.try_wait() {Ok(Some(code))=>{warn!("警告: {} 已退出（{}），游戏还在运行，未清理其他服务。日志: {}",s.name,code,self.log_dir.join(format!("{}.log",s.name)).display());s.reported=true;},Err(e)=>{warn!("无法读取 {} 状态: {e}",s.name);s.reported=true;},_=>{}} } }
    }
    fn cleanup(&mut self) -> Result<(),String> {
        let mut problems=Vec::new();
        for s in self.servers.iter_mut().rev() {
            match s.child.try_wait() {
                Ok(Some(_))=>continue,
                Err(e)=>{problems.push(format!("无法确认 {} 状态，不操作: {e}",s.name));continue},
                Ok(None)=>{}
            }
            let pid=s.child.id(); say!("正在结束本次启动的 {}（PID {pid}）",s.name);
            // The services have independent, windowless consoles. They cannot receive
            // a Ctrl-Break from this console; only terminate the child handle we own.
            match s.child.kill() {
                Ok(()) => { let _=s.child.wait(); },
                Err(e) => problems.push(format!("结束 {} 失败，已保留进程: {e}",s.name)),
            }
        }
        for tail in self.tails.drain(..) {tail.finish();}
        while let Some(f)=self.files.pop() {if let Err(e)=f.remove_if_unchanged(){problems.push(e);}}
        if problems.is_empty(){Ok(())}else{Err(problems.join("；"))}
    }
    fn cleanup_with_cause(&mut self,cause:String)->String {
        match self.cleanup(){Ok(())=>cause,Err(e)=>format!("{cause}；清理未完全完成: {e}")}
    }
}

fn run(o:&mut Options)->Result<(),String> {
    let mut rt=Runtime::new(&o.logs)?;
    if let Some(config)=&o.config {say!("路径配置: {}",config.display());}
    let ports=preflight(o)?;
    say!("服务端目录: {}\n游戏目录: {}",o.root.display(),o.game.display());
    say!("所需端口当前空闲: {:?}",ports_list(&ports));
    let server=o.root.join("Server");
    let start=(|| -> Result<(),String> {
        rt.spawn("gameserver",&server.join("gameserver.exe"),&server)?; rt.wait_ready("gameserver",ports.game)?;
        if let Some(p)=ports.gm {rt.wait_ready("gameserver",p)?;}
        rt.spawn("sdkserver",&server.join("sdkserver.exe"),&server)?; rt.wait_ready("sdkserver",ports.sdk)?;
        rt.spawn("muipserver",&server.join("muipserver.exe"),&server)?; rt.wait_ready("muipserver",ports.muip)?;
        if !game_processes()?.is_empty(){return Err("启动服务时发现已有游戏进程，取消启动补丁".into());}
        rt.copy_patches(&o.root,&o.game)?; Ok(())
    })();
    if let Err(e)=start {warn!("清理原因：服务或补丁准备阶段失败");return Err(rt.cleanup_with_cause(e));}
    let helper=match rt.spawn_helper(&o.game) {Ok(x)=>x,Err(e)=>{warn!("清理原因：补丁启动失败");return Err(rt.cleanup_with_cause(e))}};
    say!("正在等待游戏进程，补丁进程退出不代表游戏退出……");
    let deadline=Instant::now()+Duration::from_secs(o.game_timeout);
    let mut game=None;
    let mut last_query_error=None;
    while Instant::now()<deadline {
        match game_processes() {
            Ok(ps)=>for p in ps.into_iter().filter(|p|p.parent==helper) {
                match win::WatchedGame::open(p.pid,&o.game.join("reverse1999.exe")) {Ok(g)=>{game=Some(g);break},Err(e)=>last_query_error=Some(e.to_string())}
            },
            Err(e)=>last_query_error=Some(e),
        }
        if game.is_some(){break;}
        if let Ok(Some(status))=rt.helper_exited(){if !status.success() && game_processes().map(|p|p.is_empty()).unwrap_or(false) {warn!("清理原因：补丁进程返回失败且没有游戏进程");let cause=format!("补丁进程启动失败（{status}），日志: {}",rt.log_dir.join("launcher.log").display());return Err(rt.cleanup_with_cause(cause));}}
        thread::sleep(Duration::from_millis(50));
    }
    let game=match game {
        Some(g)=>g,
        None=>{
            let no_game=game_processes().map(|p|p.is_empty()).unwrap_or(false);
            let helper_done=rt.helper_exited().map(|v|v.is_some()).unwrap_or(false);
            if no_game && helper_done {warn!("清理原因：补丁已退出且未检测到游戏");let cause=format!("未检测到游戏启动，日志: {}",rt.log_dir.join("launcher.log").display());return Err(rt.cleanup_with_cause(cause));}
            return Err(format!("无法安全确认游戏进程，服务和复制的补丁已保留，请手动检查。详情: {}",last_query_error.unwrap_or_else(||"可能仍在启动".into())));
        }
    };
    match guardian::spawn(&rt,&o,game.pid,helper) {
        Ok(())=>say!("后台监护已就绪：即使窗口提前关闭，也会等待游戏退出后清理本次服务。"),
        Err(e)=>warn!("后台监护未能启动：{e}。请保留启动器窗口，直到游戏退出。"),
    }
    say!("游戏已启动（PID {}）。此窗口会保持打开，直到游戏退出。",game.pid);
    loop {
        match game.is_running() {Ok(false)=>break, Ok(true)=>{},Err(e)=>return Err(format!("无法确认游戏状态，服务和补丁已保留: {e}"))}
        rt.report_dead_services();
        thread::sleep(Duration::from_millis(400));
    }
    say!("检测到游戏进程退出，检查是否可以安全清理……");
    let still_games=game_processes()?;
    if !still_games.is_empty() {return Err(format!("仍有其他游戏进程运行（PID {:?}），服务和补丁已保留",still_games.iter().map(|p|p.pid).collect::<Vec<_>>()));}
    if rt.helper_exited()?.is_none() {return Err("补丁进程仍在运行，服务和补丁已保留".into());}
    say!("清理原因：本次游戏进程已退出");
    // Give any final asynchronous database work a short opportunity to finish.
    thread::sleep(Duration::from_secs(3));
    rt.cleanup().map_err(|e|format!("清理未完全完成: {e}"))?;
    say!("完成。服务日志保存在: {}",rt.log_dir.display());
    Ok(())
}

fn main() {
    if env::args_os().nth(1).as_deref()==Some(std::ffi::OsStr::new("--guardian")) {
        if let Err(e)=guardian::run() {warn!("后台监护退出: {e}");std::process::exit(1);}
        return;
    }
    let mut pause=true;
    let result=match Options::parse() {Ok(Some(mut o))=>{pause=o.pause;run(&mut o)},Ok(None)=>{pause=false;Ok(())},Err(e)=>Err(e)};
    let failed=result.is_err(); if let Err(e)=result {warn!("\n启动器未能完成: {e}");}
    if pause {say!("\n按 Enter 关闭此窗口……"); let _=io::stdin().read_line(&mut String::new());}
    if failed {std::process::exit(1)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn config_extracts_ports() {
        let p=env::temp_dir().join(format!("r1999-port-test-{}.toml",std::process::id()));
        fs::write(&p,"[server]\ngame_port = 23301\nhttp_port = 21000\n[muip]\nport = 21100\n[muip_gm]\nenabled = true\nport = 21101\n").unwrap();
        let cfg=read_ports(&p).unwrap(); assert_eq!(ports_list(&cfg),vec![23301,21000,21100,21101]); fs::remove_file(p).unwrap();
    }
    #[test] fn frees_listener_in_test_child() {
        let probe=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port=probe.local_addr().unwrap().port();
        drop(probe);
        let script=format!("$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,{port}); $l.Start(); Start-Sleep -Seconds 30");
        let mut child=std::process::Command::new("powershell.exe").args(["-NoProfile","-Command",&script]).spawn().unwrap();
        let result=(|| {
            let deadline=Instant::now()+Duration::from_secs(10);
            while Instant::now()<deadline {
                if win::listeners().unwrap().contains(&(port,child.id())) {return ensure_ports_free(&[port]);}
                std::thread::sleep(Duration::from_millis(50));
            }
            Err("测试监听进程未启动".into())
        })();
        let _=child.kill(); let _=child.wait();
        result.unwrap();
        assert!(!win::listeners().unwrap().iter().any(|(p,_)|*p==port));
    }
    #[test] fn listeners_match_owned_port() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port=listener.local_addr().unwrap().port();
        assert!(win::listeners().unwrap().iter().any(|(p,pid)|*p==port && *pid==std::process::id()));
    }
    #[test] fn tail_shows_complete_lines_and_flushes_last_line() {
        use std::sync::mpsc;
        let path=env::temp_dir().join(format!("r1999-tail-test-{}.log",std::process::id()));
        let mut writer=OpenOptions::new().create_new(true).write(true).open(&path).unwrap();
        let reader=File::open(&path).unwrap();
        let (tx,rx)=mpsc::channel();
        let tail=LogTail::start(reader,move|line|{tx.send(line).unwrap();}).unwrap();
        let bytes="出招：第一张".as_bytes();
        writer.write_all(&bytes[..2]).unwrap();writer.flush().unwrap();
        thread::sleep(Duration::from_millis(150));
        assert!(rx.try_recv().is_err());
        writer.write_all(&bytes[2..]).unwrap();writer.write_all(b"\r\n").unwrap();writer.flush().unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(),"出招：第一张");
        writer.write_all(b"last line").unwrap();writer.flush().unwrap();
        thread::sleep(Duration::from_millis(150));
        tail.finish();
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(),"last line");
        drop(writer);
        say!("准备删除测试日志: {}",path.display());
        fs::remove_file(&path).unwrap();
    }

    #[test] fn extracts_only_real_error_identifiers() {
        let line="ERROR gameserver: InvalidSkillDefinition { skill_id: 30941124, issues: [RuleIssue { effect_id: 30941124 }] }";
        assert_eq!(error_identifiers(line),vec![("技能 ID",30941124),("效果 ID",30941124)]);
        let line="ERROR skill_effect_id=31420121 card_id=42";
        assert_eq!(error_identifiers(line),vec![("技能效果 ID",31420121),("卡牌 ID",42)]);
        assert!(error_identifiers("INFO skill_id=123").is_empty());
        assert!(error_identifiers("ERROR no context").is_empty());
        assert!(error_identifiers("ERROR unrelated_skill_id=88").is_empty());
    }

    #[test] fn records_normal_operation_context_without_guessing_skills() {
        let mut trace=BattleTrace::default();
        assert!(trace.observe("2026-01-01 INFO gameserver: Received Cmd: SetHeroGroupSnapshotCmd").is_some());
        assert!(trace.observe("2026-01-01 INFO gameserver: Received Cmd: StartAct229BattleCmd").is_some());
        assert!(trace.observe("2026-01-01 INFO gameserver: Received Cmd: UseClothSkillCmd").is_some());
        assert!(trace.observe("2026-01-01 INFO gameserver: Received Cmd: BeginRoundCmd").is_some());
        assert!(trace.observe("2026-01-01 INFO gameserver: Received Cmd: GetServerTimeCmd").is_none());
        assert!(trace.observe("2026-01-01 INFO battle: skill_id=123").is_some());
        let context=trace.context().join("\n");
        assert!(context.contains("StartAct229BattleCmd"));
        assert!(context.contains("UseClothSkillCmd"));
        assert!(context.contains("BeginRoundCmd"));
        assert!(context.contains("skill_id=123"));
        assert!(!context.contains("GetServerTimeCmd"));
        assert!(context.contains("具体出牌未知"));
        for _ in 0..45 {trace.observe("INFO gameserver: Received Cmd: BeginRoundCmd");}
        assert_eq!(trace.recent.len(),40);
        assert!(trace.context().join("\n").contains("StartAct229BattleCmd"));
        assert_eq!(plain_service_line("\u{1b}[32mINFO\u{1b}[0m"),"INFO");
    }

    #[test] fn ini_paths_are_relative_to_the_config_not_the_working_directory() {
        let base=PathBuf::from(r"D:\示例 目录");
        let cfg=PathConfig::parse("\u{feff}# 注释\n[paths]\nserver_root=.\ngame_dir=\"..\\游戏=新目录\"\nlog_dir=logs\n",&base).unwrap();
        assert_eq!(cfg.root,Some(base.join(".")));
        assert_eq!(cfg.game,Some(base.join(r"..\游戏=新目录")));
        assert_eq!(cfg.logs,Some(base.join("logs")));
    }
    #[test] fn ini_rejects_bad_keys_and_missing_values() {
        let base=Path::new(r"D:\example");
        assert!(PathConfig::parse("[paths]\ngame_dr=C:\\game",base).is_err());
        assert!(PathConfig::parse("[paths]\ngame_dir=",base).is_err());
        assert!(PathConfig::parse("game_dir=a\ngame_dir=b",base).is_err());
    }

    #[test] fn terminal_colors_do_not_hide_error_identifiers() {
        let raw="\u{1b}[31mERROR\u{1b}[0m battle: \u{1b}[3mskill_id\u{1b}[0m\u{1b}[2m=\u{1b}[0m12345 \u{1b}[3meffect_id\u{1b}[0m\u{1b}[2m=\u{1b}[0m6789";
        let cleaned=plain_service_line(raw);
        assert_eq!(cleaned,"ERROR battle: skill_id=12345 effect_id=6789");
        assert_eq!(error_identifiers(&cleaned),vec![("技能 ID",12345),("效果 ID",6789)]);
    }

}
