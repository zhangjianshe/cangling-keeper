# 昇腾 NPU 集成 k3s（Ascend 310P）

本文档说明如何把昇腾 NPU（Ascend 310P）接入 k3s 集群，让 Pod 以 `huawei.com/Ascend310P` 为资源名申请 NPU。

## 1. 架构

华为 MindX DL 的 NPU 容器化方案由两层组成：

| 组件 | 作用 | 部署位置 |
| --- | --- | --- |
| **Ascend Docker Runtime**（`ascend-docker-runtime`） | OCI 运行时包装器：在容器 prestart 阶段读取 `ASCEND_VISIBLE_DEVICES`，把对应 `/dev/davinci*` 设备 + CANN 驱动库挂载进容器并设置 device cgroup | 注册到 k3s 内嵌 containerd |
| **Ascend Device Plugin**（`ascend-device-plugin`） | DaemonSet，发现 NPU、上报 `huawei.com/Ascend310P` 资源、健康检查；分配时给容器注入 `ASCEND_VISIBLE_DEVICES` 环境变量（`-useAscendDocker=true`） | 集群 DaemonSet |

关键点：device-plugin **只注入环境变量，不挂设备**；真正的设备挂载由 `ascend-docker-runtime` 完成。因此必须把 `ascend` 注册为 containerd 的**默认运行时**（未指定 runtimeClassName 的 Pod 都走它；无 `ASCEND_VISIBLE_DEVICES` 时它是 runc 的透传）。

## 2. 硬件 / 软件前置

- 昇腾卡：Atlas 300I / 310P 系列（`/dev/davinci*` 存在）
- CANN 驱动 + toolkit 已安装（`/usr/local/Ascend/driver`、`/etc/ascend_install.info`）
- k3s-agent 已安装并加入集群（本仓库 `kylin-arm/k3s-agent` 包，v1.30.13-rc1+k3s1，内嵌 containerd v1.7.27）

离线包在 `cangling-repo/kylin-arm/npu/`，内容：

```
npu/
  install.sh                                        # 节点侧安装脚本
  ascend-docker-runtime_6.0.RC3_linux-aarch64.run    # 昇腾容器运行时
  ascend-k8sdeviceplugin-v6.0.RC3-arm64.tar          # 预构建 device-plugin 镜像
  device-plugin-310P-v6.0.RC3.yaml                   # DaemonSet 清单
  ascend-mindxdl-device-plugin_6.0.RC3_linux-aarch64.zip  # 源码包（重建镜像用）
```

## 3. 节点侧安装（NPU 工作节点）

```bash
cd npu
bash install.sh
```

脚本完成：

1. 安装 `ascend-docker-runtime`（已安装则跳过）
2. 写 `/var/lib/rancher/k3s/agent/etc/containerd/config.toml.tmpl`，注册 `ascend` 为默认运行时
3. 导入 device-plugin 镜像到 k3s containerd（`imagePullPolicy=Never`）
4. 重启 `k3s-agent`

> **为什么写 `config.toml.tmpl` 而不是 `config.toml`**：k3s 每次启动都会从 `config.toml.tmpl` 重新生成 `config.toml`，直接改 `config.toml` 会被覆盖。
>
> **为什么手动写模板**：`ascend-docker-runtime` 自带的 `ascend-docker-plugin-install-helper` 只识别 containerd v1 配置（`io.containerd.runtime.v1.linux`），而 k3s 1.30 使用 containerd v2 配置（`version = 2`），所以必须手动写模板注册运行时。

## 4. 集群侧步骤（主节点）

```bash
# 1) 给 NPU 节点打标签（DaemonSet 的 nodeSelector 要求）
kubectl label node <NPU节点名> accelerator=huawei-Ascend310P

# 2) 部署 device-plugin
kubectl apply -f device-plugin-310P-v6.0.RC3.yaml

# 3) 验证
kubectl -n kube-system get pods -l name=ascend-device-plugin-ds   # Running
kubectl describe node <NPU节点名> | grep -A2 Ascend310P           # 12 huawei.com/Ascend310P
```

## 5. 验证 NPU 分配

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: npu-test
spec:
  nodeSelector:
    kubernetes.io/hostname: <NPU节点名>
  containers:
  - name: npu-test
    image: ascend-k8sdeviceplugin:v6.0.RC3
    imagePullPolicy: Never
    command: ["/bin/bash", "-c"]
    args: ["echo ASCEND_VISIBLE_DEVICES=$ASCEND_VISIBLE_DEVICES; ls -l /dev/davinci*"]
    resources:
      limits:
        huawei.com/Ascend310P: 1
```

预期日志形如：

```
ASCEND_VISIBLE_DEVICES=7
crw-rw-rw- 1 1001 1001 235, 7 ... /dev/davinci7
crw-rw-rw- 1 1001 1001 236, 0 ... /dev/davinci_manager
```

## 6. 当前集群状态（已实施）

| 项 | 值 |
| --- | --- |
| 主节点 | `hn` = 10.141.8.61（`localhost.localdomain`，control-plane,master） |
| NPU 节点 | `hn-gpu01` = 10.141.8.62（集群节点名 `localhost.localdomain-269cb157`） |
| NPU | 6× Ascend 310P3 卡 = 12 个芯片（`/dev/davinci0`…`11`） |
| 驱动 / toolkit | CANN 24.1.1.3 / 8.1.RC1 |
| 运行时 | 已注册 `ascend` 为默认运行时（节点已内置 v6.0.0.SPC1，脚本对空节点装 v6.0.RC3） |
| device-plugin | DaemonSet `ascend-device-plugin310p-daemonset` 运行中，上报 12 个 `huawei.com/Ascend310P` |

## 7. 常见问题

- **`exec /bin/bash: exec format error`**：镜像架构与节点不符。`ascend-k8sdeviceplugin` 镜像必须按 `linux/arm64` 构建（在 x86 机器上需 `docker build --platform linux/arm64`）。
- **节点 NotReady**：重启 k3s-agent 后短暂 NotReady 属正常，kubelet 重新注册即可恢复。
- **device-plugin 日志 `get device ip failed ... -8255`**：310P 走 PCIe、无设备 IP，属无害告警。
- **镜像不可拉取**：`imagePullPolicy: Never`，必须先用 `k3s ctr -n k8s.io images import <tar>` 导入。

## 8. 重建 device-plugin 镜像（可选）

```bash
unzip ascend-mindxdl-device-plugin_6.0.RC3_linux-aarch64.zip -d dp
cd dp && cp device-plugin Dockerfile faultCode.json faultCustomization.json SwitchFaultCode.json .
docker pull --platform linux/arm64 ubuntu:18.04
docker build --platform linux/arm64 -t ascend-k8sdeviceplugin:v6.0.RC3 .
docker save ascend-k8sdeviceplugin:v6.0.RC3 -o ascend-k8sdeviceplugin-v6.0.RC3-arm64.tar
```
