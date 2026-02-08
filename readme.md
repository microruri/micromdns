# micromdns

一个基于 `libmdns` 的简单 mDNS 广播程序，用于在局域网内广播 `<name>.local -> 本机 IP`。

## 重要提示

本项目当前代码由 **GPT-5.3-Codex** 生成，**可能存在错误或边界情况未覆盖**。  
请在生产环境使用前自行审查、测试并按需修改。

## 功能

- 广播主机名：`<name>.local`
- 广播地址：本机非 loopback 网卡 IP（支持按接口过滤）
- 接口变化自动更新：当网卡/IP 发生变化时，自动重建 mDNS responder
- 终端日志输出：包含 `info` 和 `debug` 级别关键信息

## 依赖

- Rust（建议 stable）
- Cargo

## 构建

```bash
cargo build
```

## 运行

```bash
# 方式 1：位置参数 name
cargo run -- myname

# 方式 2：显式参数
cargo run -- --name myname
```

程序会广播 `myname.local`。

## 命令行参数

```text
Usage:
  mdnsd --name <name> [--interface <iface> ...]
  mdnsd <name> [--interface <iface> ...]

Options:
  -n, --name <name>           Host name, resolves as <name>.local
  -i, --interface <iface>     Interface name, repeatable. Default is '*' (all)
  -h, --help                  Show this help
```

### 接口过滤示例

```bash
# 仅使用 Wi-Fi
cargo run -- --name myname -i "Wi-Fi"

# 使用多个接口（重复 -i）
cargo run -- --name myname -i "Ethernet" -i "Wi-Fi"

# 明确表示所有接口（默认也是这个）
cargo run -- --name myname -i "*"
```

## 日志说明

程序会输出类似日志：

```text
[INFO ] config loaded: name=myname, interfaces=*
[DEBUG] initial interface snapshot=[...]
[INFO ] starting mdns responder: hostname=myname.local, interfaces=*, visible_ips=[...]
[DEBUG] responder allowed_ips=[...]
```

当检测到网卡变化时会输出：

```text
[INFO ] network interface change detected, restarting mdns responder
[INFO ] mdns responder restarted
```

## 实现说明（简要）

- 通过 `libmdns::Responder::with_default_handle_and_ip_list_and_hostname(...)` 启动 responder
- 周期轮询（当前 3 秒）比较接口快照
- 若快照变化，则释放旧 responder 并重新创建

## 已知限制

- 主要面向“简单可用”场景，未实现复杂冲突处理与高级 DNS-SD 功能
- 接口变化检测使用轮询，不是系统事件订阅
