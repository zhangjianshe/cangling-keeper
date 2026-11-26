#!/usr/bin/env python3
"""Remove only Keeper's manual systemd service; retain all user data."""
import argparse
import os
from pathlib import Path
import subprocess

UNITS = ('cangling-keeper-local.service', 'cangling-keeper-http.socket', 'cangling-keeper-https.socket')
UNIT_DIR = Path('/etc/systemd/system')
RULE = Path('/etc/polkit-1/rules.d/49-cangling-keeper-local.rules')

def main():
    parser = argparse.ArgumentParser(description='卸载 Keeper 系统服务，保留程序、配置和会话数据')
    parser.add_argument('binary', help=argparse.SUPPRESS)
    parser.add_argument('--check', action='store_true', help='仅列出卸载范围，不修改系统')
    args = parser.parse_args()
    paths = [UNIT_DIR / name for name in UNITS] + [RULE]
    if args.check:
        print('将停止并移除以下服务配置（保留所有用户数据）：')
        for path in paths:
            try:
                state = ' [已存在]' if path.exists() or path.is_symlink() else ' [未安装]'
            except PermissionError:
                state = ' [需管理员权限检查]'
            print(str(path) + state)
        return
    if os.geteuid() != 0:
        parser.error('卸载需要管理员权限，请运行 cangling-keeper uninstall-service')
    # Query loaded state as well as files: a prior partial uninstall can leave units loaded.
    loaded = []
    for name in UNITS:
        result = subprocess.run(['systemctl', 'show', name, '--property=LoadState', '--value'],
                                check=True, capture_output=True, text=True)
        if result.stdout.strip() != 'not-found':
            loaded.append(name)
    if loaded:
        # Stop sockets and service together before removing authorization or unit files.
        subprocess.run(['systemctl', 'stop', *loaded], check=True)
        subprocess.run(['systemctl', 'disable', *loaded], check=True)
    for path in paths:
        path.unlink(missing_ok=True)
    subprocess.run(['systemctl', 'daemon-reload'], check=True)
    print('卸载完成：已移除 Keeper 服务及端口监听配置；程序、证书、配置和会话数据均保留。')

if __name__ == '__main__':
    main()
