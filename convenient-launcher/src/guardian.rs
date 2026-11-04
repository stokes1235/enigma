use std::{env,ffi::OsString,fs::OpenOptions,io::Write,os::windows::process::CommandExt,path::PathBuf,process::{Command,Stdio},thread,time::{Duration,Instant,UNIX_EPOCH}};
use crate::{emit,game_processes,win,Options,OwnedFile,Runtime,LAUNCH_LOG,INDEPENDENT_PROCESS};

struct Args {
    parent:u32, game:u32, helper:u32, game_dir:PathBuf, log_dir:PathBuf,
    services:Vec<(u32,PathBuf)>, files:Vec<OwnedFile>,
}
fn next(args:&mut impl Iterator<Item=OsString>)->Result<OsString,String> {args.next().ok_or_else(||"后台监护参数不完整".into())}
fn number<T:std::str::FromStr>(args:&mut impl Iterator<Item=OsString>)->Result<T,String> {
    next(args)?.to_string_lossy().parse().map_err(|_|"后台监护参数无效".into())
}
fn parse()->Result<Args,String> {
    let mut a=env::args_os().skip(2);
    let parent=number(&mut a)?; let game=number(&mut a)?; let helper=number(&mut a)?;
    let game_dir=PathBuf::from(next(&mut a)?); let log_dir=PathBuf::from(next(&mut a)?);
    let n:usize=number(&mut a)?; if n!=3 {return Err("后台监护的服务数量不正确".into());}
    let mut services=Vec::new();
    for _ in 0..n {services.push((number(&mut a)?,PathBuf::from(next(&mut a)?)));}
    let n:usize=number(&mut a)?; if n>2 {return Err("后台监护的临时文件数量不正确".into());}
    let mut files=Vec::new();
    for _ in 0..n {
        let path=PathBuf::from(next(&mut a)?);
        let id=win::FileId{volume:number(&mut a)?,index_high:number(&mut a)?,index_low:number(&mut a)?};
        let size=number(&mut a)?; let nanos:u64=number(&mut a)?;
        let modified=UNIX_EPOCH.checked_add(Duration::from_nanos(nanos)).ok_or("文件时间无效")?;
        files.push(OwnedFile{path,id,size,modified});
    }
    if a.next().is_some(){return Err("后台监护参数过多".into());}
    Ok(Args{parent,game,helper,game_dir,log_dir,services,files})
}
fn ioerr(what:&str,e:impl std::fmt::Display)->String {format!("{what}: {e}")}

pub fn spawn(rt:&Runtime,options:&Options,game:u32,helper:u32)->Result<(),String> {
    let mut command=Command::new(env::current_exe().map_err(|e|ioerr("定位启动器失败",e))?);
    command.arg("--guardian").arg(std::process::id().to_string()).arg(game.to_string()).arg(helper.to_string())
        .arg(&options.game).arg(&rt.log_dir).arg(rt.servers.len().to_string());
    for service in &rt.servers {
        command.arg(service.child.id().to_string()).arg(options.root.join("Server").join(format!("{}.exe",service.name)));
    }
    command.arg(rt.files.len().to_string());
    for file in &rt.files {
        let nanos=file.modified.duration_since(UNIX_EPOCH).map_err(|e|ioerr("读取临时文件时间失败",e))?.as_nanos();
        let nanos=u64::try_from(nanos).map_err(|_|"临时文件时间超出范围")?;
        command.arg(&file.path).arg(file.id.volume.to_string()).arg(file.id.index_high.to_string())
            .arg(file.id.index_low.to_string()).arg(file.size.to_string()).arg(nanos.to_string());
    }
    let mut child=command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(INDEPENDENT_PROCESS).spawn().map_err(|e|ioerr("启动后台监护失败",e))?;
    let ready=rt.log_dir.join("guardian.ready"); let deadline=Instant::now()+Duration::from_secs(8);
    loop {
        if std::fs::read(&ready).map(|v|v==b"ready\n").unwrap_or(false){return Ok(());}
        if let Some(status)=child.try_wait().map_err(|e|ioerr("读取后台监护状态失败",e))? {
            return Err(format!("后台监护提前退出（{status}），请查看 guardian.log"));
        }
        if Instant::now()>deadline {
            let _=child.kill();let _=child.wait();
            return Err("等待后台监护就绪超时，请查看 guardian.log".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn run()->Result<(),String> {
    let args=parse()?;
    let logfile=OpenOptions::new().create_new(true).write(true).open(args.log_dir.join("guardian.log"))
        .map_err(|e|ioerr("创建后台监护日志失败",e))?;
    LAUNCH_LOG.set(std::sync::Mutex::new(logfile)).map_err(|_|"后台监护日志已初始化")?;
    let parent=win::WatchedProcess::open(args.parent,None,false).map_err(|e|ioerr("无法确认启动器进程",e))?;
    let game=win::WatchedGame::open(args.game,&args.game_dir.join("reverse1999.exe")).map_err(|e|ioerr("无法确认游戏进程",e))?;
    let mut services=Vec::new();
    for (pid,path) in &args.services {
        let process=win::WatchedProcess::open(*pid,Some(path),true).map_err(|e|ioerr("无法确认服务进程",e))?;
        services.push(process);
    }
    let entries=win::processes().map_err(|e|ioerr("无法检查补丁进程",e))?;
    let helper=match entries.iter().find(|p|p.pid==args.helper) {
        None=>None,
        Some(p) if p.parent==args.parent=>{
            // The patch launcher may have already exited while its process ID is
            // still visible. A terminated process can no longer report its image.
            match win::WatchedProcess::open(args.helper,None,false) {
                Ok(handle) if !handle.is_running().map_err(|e|ioerr("无法确认补丁进程状态",e))? => None,
                Ok(handle) => match win::WatchedProcess::open(args.helper,Some(&args.game_dir.join("launcher.exe")),false) {
                    Ok(verified)=>Some(verified),
                    Err(_e) if !handle.is_running().map_err(|x|ioerr("无法确认补丁进程状态",x))? => None,
                    Err(e)=>return Err(ioerr("无法确认补丁进程",e)),
                },
                Err(e)=>{
                    if win::processes().map_err(|x|ioerr("无法检查补丁进程",x))?.iter().any(|p|p.pid==args.helper) {
                        return Err(ioerr("无法确认补丁进程",e));
                    }
                    None
                }
            }
        },
        Some(_)=>return Err("补丁进程身份已变化，不执行后台清理".into()),
    };
    let mut ready=OpenOptions::new().create_new(true).write(true).open(args.log_dir.join("guardian.ready"))
        .map_err(|e|ioerr("后台监护无法标记就绪",e))?;
    ready.write_all(b"ready\n").and_then(|_|ready.flush()).map_err(|e|ioerr("后台监护无法写入就绪标记",e))?;
    say!("后台监护就绪；等待启动器结束。游戏 PID: {}",args.game);
    parent.wait().map_err(|e|ioerr("等待启动器结束失败",e))?;
    say!("启动器已退出；等待游戏本体结束。");
    game.wait().map_err(|e|ioerr("等待游戏结束失败",e))?;
    if let Some(helper)=helper {helper.wait().map_err(|e|ioerr("等待补丁结束失败",e))?;}
    loop {
        let games=game_processes()?;
        if games.is_empty(){break;}
        say!("仍有游戏进程运行，后台监护继续等待。");
        thread::sleep(Duration::from_secs(5));
    }
    thread::sleep(Duration::from_secs(3));
    if !game_processes()?.is_empty(){return Err("发现新的游戏进程，保留本次服务和临时文件".into());}
    let mut problems=Vec::new();
    for service in services.into_iter().rev() {
        if !game_processes()?.is_empty(){return Err("发现新的游戏进程，停止后台清理".into());}
        if service.is_running().map_err(|e|ioerr("确认服务状态失败",e))? {
            say!("正在结束本次启动的服务 PID {}",service.pid);
            if let Err(e)=service.terminate(){problems.push(format!("服务 PID {} 清理失败: {e}",service.pid));}
        }
    }
    if !game_processes()?.is_empty(){return Err("发现新的游戏进程，保留临时文件".into());}
    for file in args.files.into_iter().rev() {if let Err(e)=file.remove_if_unchanged(){problems.push(e);}}
    if problems.is_empty(){say!("后台监护清理完成。");Ok(())}else{Err(problems.join("；"))}
}
