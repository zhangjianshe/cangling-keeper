#!/usr/bin/env python3
"""Install manual-start systemd listeners; run once with sudo, never enable units."""
import os, pwd, sys, subprocess, pathlib, datetime, shutil, json, tempfile, argparse
parser = argparse.ArgumentParser(description="安装 Keeper 手动启动服务（不启用开机自启）")
parser.add_argument('binary')
parser.add_argument('--http-port', type=int, default=80)
parser.add_argument('--https-port', type=int, default=443)
parser.add_argument('--check', action='store_true', help='仅预检，不修改系统')
args=parser.parse_args()
if not all(1 <= p <= 65535 for p in (args.http_port,args.https_port)):
    parser.error('端口必须在 1–65535 之间')
if args.http_port == args.https_port: parser.error('HTTP 和 HTTPS 端口不能相同')
dry_run=args.check
if not dry_run and (os.geteuid() != 0 or not os.environ.get("SUDO_USER") or os.environ["SUDO_USER"] == "root"):
    sys.exit("请从普通用户运行 cangling-keeper install-service")
user = pwd.getpwnam(os.environ["SUDO_USER"]) if os.environ.get("SUDO_USER") else pwd.getpwuid(os.getuid())
source = pathlib.Path(args.binary).resolve(strict=True)
exe = pathlib.Path("/opt/cangling-keeper/cangling-keeper")
repo = exe.parent
command_link = pathlib.Path("/usr/local/bin/cangling-keeper")
if command_link.exists() or command_link.is_symlink():
    if not command_link.is_symlink() or command_link.readlink() != exe:
        sys.exit("命令入口已存在且不是 Keeper 安装链接，未覆盖："+str(command_link))
def quote(s): return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%") + '"'
files = {}
for name, port in [("http",args.http_port),("https",args.https_port)]:
    files[f"/etc/systemd/system/cangling-keeper-{name}.socket"] = f"""[Unit]
Description=Keeper {name} listener (manual start)
PartOf=cangling-keeper-local.service
[Socket]
ListenStream=127.0.0.1:{port}
Accept=no
FileDescriptorName={name}
Service=cangling-keeper-local.service
"""
files["/etc/systemd/system/cangling-keeper-local.service"] = f"""[Unit]
Description=Cangling Keeper (unprivileged, manual start)
Requires=cangling-keeper-http.socket cangling-keeper-https.socket
After=cangling-keeper-http.socket cangling-keeper-https.socket
[Service]
Type=simple
User={user.pw_name}
Group={user.pw_gid}
WorkingDirectory={str(repo).replace("%", "%%")}
Environment={quote('HOME='+user.pw_dir)}
Environment={quote('XDG_RUNTIME_DIR=/run/user/'+str(user.pw_uid))}
Environment={quote('DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(user.pw_uid)+'/bus')}
Environment=KEEPER_SYSTEMD_SERVICE=cangling-keeper-local.service
ExecStart={quote(exe)} --no-open --listen 127.0.0.1:{args.http_port} --https-listen 127.0.0.1:{args.https_port}
Sockets=cangling-keeper-http.socket cangling-keeper-https.socket
Restart=on-failure
RestartSec=2
TimeoutStopSec=10
KillMode=control-group
"""
files["/etc/polkit-1/rules.d/49-cangling-keeper-local.rules"] = """polkit.addRule(function(action, subject) {
  if (action.id === "org.freedesktop.systemd1.manage-units" && subject.user === USER &&
      ["cangling-keeper-local.service", "cangling-keeper-http.socket", "cangling-keeper-https.socket"].indexOf(action.lookup("unit")) >= 0 &&
      ["start", "stop", "restart"].indexOf(action.lookup("verb")) >= 0) return polkit.Result.YES;
});
""".replace("USER", json.dumps(user.pw_name))
# Validate the complete generated unit set before changing system files.
with tempfile.TemporaryDirectory(prefix="keeper-systemd-check-") as staging:
    units=[]
    for name, content in files.items():
        if name.endswith((".socket", ".service")):
            path=pathlib.Path(staging)/pathlib.Path(name).name
            # Validate against the existing source executable before the first install.
            path.write_text(content.replace(quote(exe), quote(source)))
            units.append(str(path))
    subprocess.run(['systemd-analyze', 'verify']+units, check=True)
if dry_run:
    print("配置预检通过；程序目标 /opt/cangling-keeper/cangling-keeper，命令入口 /usr/local/bin/cangling-keeper；未修改系统配置。")
    sys.exit(0)
backup=pathlib.Path('/var/backups/keeper-socket-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
backup.mkdir(mode=0o700)
# Publish a fresh inode atomically, allowing an existing process to keep running.
if repo.is_symlink(): sys.exit("安装目录不能是符号链接")
repo.mkdir(mode=0o755, parents=True, exist_ok=True)
if repo.stat().st_uid != 0 or repo.stat().st_mode & 0o022:
    sys.exit("安装目录必须由 root 拥有且不可由其他用户写入")
if exe.exists(): shutil.copyfile(exe, backup/'cangling-keeper')
fd, staged = tempfile.mkstemp(prefix='.keeper-', dir=repo)
try:
    with os.fdopen(fd, 'wb') as output, source.open('rb') as input_file:
        shutil.copyfileobj(input_file, output)
        output.flush(); os.fsync(output.fileno())
    os.chmod(staged, 0o755)
    os.replace(staged, exe)
finally:
    if os.path.exists(staged): os.unlink(staged)
command_link.parent.mkdir(parents=True, exist_ok=True)
if not command_link.is_symlink(): command_link.symlink_to(exe)
for name, content in files.items():
    target=pathlib.Path(name)
    if target.exists(): shutil.copy2(target, backup/target.name)
    target.parent.mkdir(parents=True,exist_ok=True)
    target.write_text(content); target.chmod(0o644)
subprocess.run(['systemctl','daemon-reload'],check=True)
subprocess.run(['systemd-analyze','verify']+[n for n in files if n.endswith(('.socket','.service'))],check=True)
if shutil.which('getcap') and subprocess.check_output(['getcap',str(exe)]).strip():
    subprocess.run(['setcap','-r',str(exe)],check=True)
print('配置完成：未启用开机自启，也尚未启动服务。备份：'+str(backup))

# Drop privileges before touching desktop files (including existing symlinks).
desktop_script = os.environ.get('KEEPER_DESKTOP_INSTALL_SCRIPT')
if desktop_script:
    subprocess.run(['runuser','-u',user.pw_name,'--','env','HOME='+user.pw_dir,
                    'XDG_RUNTIME_DIR=/run/user/'+str(user.pw_uid),
                    'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/'+str(user.pw_uid)+'/bus',
                    'python3','-c',desktop_script,str(args.http_port)],check=True)
