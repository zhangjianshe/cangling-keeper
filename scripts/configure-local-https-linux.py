#!/usr/bin/env python3
"""Run with sudo after building: grant only low-port binding and map the local name."""
import os,sys,subprocess,shutil
from pathlib import Path
if os.geteuid()!=0:raise SystemExit('请使用 sudo 运行此脚本')
exe=Path(sys.argv[1]).resolve(strict=True)
if not exe.is_file() or exe.name!='cangling-keeper':raise SystemExit('需要 Keeper 可执行文件路径')
subprocess.run(['/usr/sbin/setcap','cap_net_bind_service=+ep',str(exe)],check=True)
p=Path('/etc/hosts');text=p.read_text();lines=[]
for line in text.splitlines():
    body,sep,comment=line.partition('#');parts=body.split()
    if len(parts)>1 and 'local.cangling.cn' in parts[1:]:
        names=[name for name in parts[1:] if name!='local.cangling.cn']
        line=(parts[0]+'\t'+' '.join(names) if names else '')+(' #'+comment if sep else '')
    if line.strip():lines.append(line)
lines.append('127.0.0.1\tlocal.cangling.cn # Cangling Keeper local HTTPS')
backup=Path('/etc/hosts.before-cangling-keeper-https')
if not backup.exists():shutil.copy2(p,backup)
p.write_text('\n'.join(lines)+'\n')
print('已配置本机域名及低端口权限。重新编译或替换程序后需要重新设置 capability。')
