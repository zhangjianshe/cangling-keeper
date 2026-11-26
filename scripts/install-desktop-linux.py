"""Run as the target desktop user, never as root."""
import os, pathlib, subprocess, sys, shlex
port=int(sys.argv[1])
home=pathlib.Path.home()
try:
    result=subprocess.run(['xdg-user-dir','DESKTOP'],check=True,capture_output=True,text=True)
    desktop=pathlib.Path(result.stdout.strip())
    if not desktop.is_absolute(): raise ValueError('invalid desktop')
except (OSError,subprocess.SubprocessError,ValueError):
    desktop=home/'Desktop'
launcher=home/'.local/bin/cangling-keeper-local'
launcher.parent.mkdir(parents=True,exist_ok=True)
launcher.write_text('''#!/bin/sh
set -eu
systemctl --no-ask-password start cangling-keeper-local.service
exec xdg-open '''+shlex.quote(f'http://127.0.0.1:{port}/')+'\n')
launcher.chmod(0o755)
def exec_quote(path):
    return '"'+str(path).replace('\\','\\\\').replace('"','\\"').replace('`','\\`').replace('$','\\$').replace('%','%%')+'"'
icon=home/'.local/share/icons/hicolor/scalable/apps/cangling-keeper.svg'
if os.environ.get('KEEPER_DESKTOP_ICON'):
    icon.parent.mkdir(parents=True,exist_ok=True)
    icon.write_text(os.environ['KEEPER_DESKTOP_ICON'])
content='[Desktop Entry]\nType=Application\nName=苍灵智能\nComment=Cangling Keeper\nExec='+exec_quote(launcher)+'\nIcon=cangling-keeper\nTerminal=false\nCategories=Utility;Network;\nStartupNotify=true\n'
apps=home/'.local/share/applications';apps.mkdir(parents=True,exist_ok=True)
desktop.mkdir(parents=True,exist_ok=True)
for directory in (apps,desktop):
    target=directory/'cangling-keeper.desktop'
    target.write_text(content);target.chmod(0o755)
    try: subprocess.run(['gio','set',str(target),'metadata::trusted','true'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    except OSError: pass
print('桌面和应用菜单快捷方式已安装：'+str(desktop/'cangling-keeper.desktop'))
