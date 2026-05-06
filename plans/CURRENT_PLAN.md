# 实施计划：HTTP/WebSocket Interaction Transport

> 状态：已实施并通过本地验证。目标是在不改变 `InteractionControlHandler` 的前提下，为 `noloong-agent` 增加可选 HTTP/WebSocket control-plane transport。stdio line-delimited JSON-RPC 继续作为默认、最低依赖、第三方 conformance baseline。

## 概览

当前 interaction control plane 已经通过 `JsonRpcHandler` 把业务 handler 和 stdio framing 分离。下一步要把这条边界进一步固化：`InteractionControlHandler` 仍只实现 `JsonRpcHandler`，transport 层负责 stdio、HTTP POST 和 WebSocket framing、auth、连接生命周期与 notification 写出。

v1 默认面向第三方 TS/Python bridge 进程：Rust host 暴露 endpoint，bridge 作为 client 主动连接。HTTP POST 只支持 request/response；需要 raw/display event notification 的 bridge 必须使用 WebSocket。Bearer token 是 transport 层的最小认证机制；细粒度动作授权仍由 `InteractionCapabilityPolicy` 和 `initialize` grant 负责。

## 架构决策

- 不修改 `InteractionControlHandler` 的公开 API、method set 或业务逻辑；只在 JSON-RPC/transport 层新增适配。
- `serve_jsonrpc` 的 stdio 行为保持兼容：一行一个 JSON-RPC request，一行一个 response/notification。
- 增加 optional feature `interaction-http`；默认 `noloong-agent` 不引入 HTTP/WebSocket 依赖。
- HTTP endpoint 只提供单次 JSON-RPC request/response，不注册 subscription，不承诺 server push。
- WebSocket endpoint 是完整双向 JSON-RPC 连接，同一 socket 承载 request、response 和 notification。
- `shutdown` 在 stdio 中结束当前 stdio server；在 WebSocket 中只关闭当前 socket，不关闭整个 listener。
- HTTP/WebSocket transport 必须支持 bearer token；未配置认证只允许测试或明确的本地嵌入场景。
- 不新增 HTTP/WS conformance runner；stdio 继续是第三方扩展/bridge 的最低 conformance transport。HTTP/WS 只新增 transport-level tests。

## 公开 API / 接口变化

- 新增 Cargo feature：
  - `interaction-http`
- 新增可选公开类型和函数：
  - `InteractionHttpTransportConfig`
  - `InteractionTransportAuth`
  - `interaction_http_router(handler, config) -> axum::Router`
  - `serve_interaction_http(listener, handler, config)`
- 新增 HTTP endpoints：
  - `POST /jsonrpc`
  - `GET /jsonrpc/ws`
- auth 约定：
  - `Authorization: Bearer <token>`
  - auth 失败返回 HTTP `401`，不进入 `JsonRpcHandler`。
- JSON-RPC 约定：
  - 仍只支持 JSON-RPC 2.0 single request object，不支持 batch。
  - JSON-RPC 协议错误返回 JSON-RPC error response。
  - HTTP POST 对 `event/subscribe` 和 `display/subscribe` 返回 structured JSON-RPC error，提示需要 bidirectional transport。

## 任务列表

### Phase 1：JSON-RPC Dispatch 基础重构

#### 任务 1：抽出单 request dispatch helper

**描述：** 从 `serve_jsonrpc` 中抽出可复用的 JSON-RPC request parse、version check、handler dispatch 和 response mapping。stdio、HTTP 和 WebSocket 都应复用同一套逻辑，避免 transport 间错误语义漂移。

**验收标准：**

- [x] `serve_jsonrpc` 外部行为不变。
- [x] 单 request helper 可以接收 `JsonRpcRequest`、`JsonRpcHandler` 和 `InteractionNotifier`，返回 `JsonRpcResponse` 与 shutdown flag。
- [x] invalid JSON、unsupported `jsonrpc` version、unknown method 和 handler error 的 response shape 与现有 stdio 测试一致。
- [x] helper 不依赖 HTTP/WebSocket 类型。

**验证：**

- [x] `cargo test -p noloong-agent --test interaction_jsonrpc`
- [x] `cargo test -p noloong-agent --test interaction_control`
- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

**依赖：** 无

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/jsonrpc.rs`
- `crates/noloong-agent/tests/interaction_jsonrpc.rs`

**预计范围：** 中

#### 任务 2：明确 notifier/outbound 连接抽象

**描述：** 将 `InteractionNotifier` 的构造和 outbound channel 管理收敛到 JSON-RPC transport 内部，使 stdio 和 WebSocket 都能创建一条 bidirectional connection；HTTP POST 则创建 request-response-only context。

**验收标准：**

- [x] bidirectional connection 有独立 outbound buffer，notification 和 response 共用同一 writer。
- [x] request-response-only context 不暴露可持久订阅能力。
- [x] `InteractionNotifier` 不需要暴露底层 channel 细节给 `InteractionControlHandler`。
- [x] `JsonRpcOutbound` 保持 crate-private 或 module-private，不进入公开 API。

**验证：**

- [x] `cargo test -p noloong-agent --test interaction_jsonrpc`
- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

**依赖：** 任务 1

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/jsonrpc.rs`

**预计范围：** 小

### Checkpoint：stdio 兼容性

- [x] `cargo fmt --check`
- [x] `cargo test -p noloong-agent --test interaction_jsonrpc`
- [x] `cargo test -p noloong-agent --test interaction_control`
- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings`

### Phase 2：HTTP/WebSocket Optional Transport

#### 任务 3：新增 feature 与依赖边界

**描述：** 在 `noloong-agent` 中增加 `interaction-http` feature，把 HTTP/WebSocket 依赖限制在 feature gate 下。默认 feature 下不能编译或拉入 axum 相关代码。

**验收标准：**

- [x] `interaction-http` feature 启用 `axum` 的 HTTP 和 WebSocket 能力。
- [x] 默认 `cargo clippy -p noloong-agent --all-targets -- -D warnings` 不需要 HTTP/WS 依赖。
- [x] 开启 feature 后 `cargo clippy -p noloong-agent --features interaction-http --all-targets -- -D warnings` 能编译。
- [x] HTTP/WS 测试需要的 client 依赖只放在 dev-dependencies 或 workspace dev 使用路径。

**验证：**

- [x] `cargo clippy -p noloong-agent --all-targets -- -D warnings`
- [x] `cargo clippy -p noloong-agent --features interaction-http --all-targets -- -D warnings`

**依赖：** 任务 1

**可能涉及文件：**

- `Cargo.toml`
- `crates/noloong-agent/Cargo.toml`
- `crates/noloong-agent/src/interaction/mod.rs`

**预计范围：** 小

#### 任务 4：实现 HTTP POST transport

**描述：** 增加 `POST /jsonrpc` endpoint。它接收单个 JSON-RPC request object，调用同一个 `JsonRpcHandler` dispatch helper，并返回单个 JSON-RPC response。

**验收标准：**

- [x] valid request 返回 JSON-RPC result response。
- [x] invalid JSON 或 unsupported JSON-RPC version 返回 JSON-RPC error response。
- [x] auth missing/wrong token 返回 HTTP `401`，不调用 handler。
- [x] request body 超过配置上限时返回 HTTP error，不调用 handler。
- [x] `event/subscribe` 和 `display/subscribe` 在 HTTP POST 上返回 JSON-RPC error，提示需要 bidirectional transport。
- [x] HTTP POST 不保留连接级 subscription 状态。

**验证：**

- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport http_post_jsonrpc_round_trips`
- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport http_auth_is_required`
- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport http_rejects_subscription_methods`

**依赖：** 任务 2、任务 3

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/http.rs`
- `crates/noloong-agent/tests/interaction_http_transport.rs`

**预计范围：** 中

#### 任务 5：实现 WebSocket transport

**描述：** 增加 `GET /jsonrpc/ws` endpoint。每个 WebSocket 连接都是一条 bidirectional JSON-RPC session：client 发送 text frame request，server 返回 response，并能在同一 socket 推送 notification。

**验收标准：**

- [x] WebSocket text frame 中的 valid JSON-RPC request 返回 response。
- [x] invalid JSON text frame 返回 parse error response，连接保持可用。
- [x] binary frame 返回 structured error 或关闭连接，行为在测试中固定。
- [x] `event/subscribe` / `display/subscribe` 可以注册 notifier，并通过同一 socket 收到 notification。
- [x] `shutdown` 返回 response 后关闭当前 WebSocket，不关闭 listener。
- [x] socket 断开时 writer task 和 outbound channel 正常退出，不泄漏 subscription writer task。

**验证：**

- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport websocket_jsonrpc_round_trips`
- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport websocket_delivers_notifications`
- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport websocket_shutdown_closes_socket_only`

**依赖：** 任务 2、任务 3

**可能涉及文件：**

- `crates/noloong-agent/src/interaction/http.rs`
- `crates/noloong-agent/tests/interaction_http_transport.rs`

**预计范围：** 中

### Checkpoint：HTTP/WS 核心能力

- [x] HTTP POST request/response tests 通过。
- [x] WebSocket request/response/notification tests 通过。
- [x] auth tests 通过。
- [x] `cargo clippy -p noloong-agent --features interaction-http --all-targets -- -D warnings`

### Phase 3：文档与集成验证

#### 任务 6：更新 interaction 文档

**描述：** 更新 interaction 文档，说明 stdio、HTTP POST 和 WebSocket 三种 transport 的能力差异，并给第三方 TS/Python bridge 作者清晰接入路径。

**验收标准：**

- [x] `INTERACTION.md` 明确 stdio 是最低依赖 transport。
- [x] `INTERACTION.md` 明确 HTTP POST 不支持 subscription notification。
- [x] `INTERACTION.md` 给出 WebSocket 连接、bearer token 和 JSON-RPC frame 示例。
- [x] 文档说明 TS/Python bridge 作为 client 主动连接 Rust host。

**验证：**

- [x] 人工检查 `crates/noloong-agent/docs/INTERACTION.md`
- [x] `rg "interaction-http|/jsonrpc|/jsonrpc/ws|Authorization" crates/noloong-agent/docs`

**依赖：** 任务 4、任务 5

**可能涉及文件：**

- `crates/noloong-agent/docs/INTERACTION.md`
- `crates/noloong-agent/docs/ARCHITECTURE.md`

**预计范围：** 小

#### 任务 7：更新验证矩阵和后续演进方向

**描述：** 更新 `CONFORMANCE_MATRIX.md` 和架构文档，把“增加 WebSocket/HTTP transport”从后续演进项移动到已验证能力，同时保留 stdio conformance baseline 的定位。

**验收标准：**

- [x] matrix 包含 HTTP POST transport tests。
- [x] matrix 包含 WebSocket notification transport tests。
- [x] `ARCHITECTURE.md` 的后续演进方向不再保留这条已完成 TODO。
- [x] 文档不暗示 HTTP/WS 已有第三方 conformance runner。

**验证：**

- [x] `rg "WebSocket|HTTP transport|stdio" crates/noloong-agent/docs/ARCHITECTURE.md crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`

**依赖：** 任务 6

**可能涉及文件：**

- `crates/noloong-agent/docs/ARCHITECTURE.md`
- `crates/noloong-agent/docs/CONFORMANCE_MATRIX.md`

**预计范围：** 小

#### 任务 8：完整验证

**描述：** 跑完整 workspace 验证，确保默认 feature、HTTP feature、stdio interaction 和已有 agent-core/provider 测试都没有回归。

**验收标准：**

- [x] 默认 feature 下 workspace 测试通过。
- [x] `interaction-http` feature 下 noloong-agent clippy 和 transport tests 通过。
- [x] stdio JSON-RPC 测试仍通过。
- [x] 不存在 `#[allow(dead_code)]`。

**验证：**

- [x] `cargo fmt --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo clippy -p noloong-agent --features interaction-http --all-targets -- -D warnings`
- [x] `cargo test --workspace`
- [x] `cargo test -p noloong-agent --features interaction-http --test interaction_http_transport`
- [x] `rg -n "#\\[allow\\(dead_code\\)\\]" crates`

**依赖：** 任务 1-7

**可能涉及文件：** 无新增实现文件

**预计范围：** 小

## 风险与缓解

| 风险 | 影响 | 缓解 |
|---|---:|---|
| HTTP POST 被误用于 event subscription | 中 | transport 层直接拒绝 subscription methods，文档明确必须使用 WebSocket |
| HTTP/WS 依赖污染默认 crate | 中 | 所有 HTTP/WS code 和 dependencies 都挂在 `interaction-http` feature 下 |
| notification 写出阻塞或丢失 | 中 | 复用 bounded outbound channel；写失败只影响当前连接，不 panic |
| WebSocket shutdown 误关整个 host | 高 | 明确 shutdown 只关闭当前 socket；listener 生命周期由 host 管理 |
| 第三方 bridge 误以为 bearer token 替代权限系统 | 中 | 文档明确 bearer token 只认证 transport，动作权限仍由 `initialize` grant 控制 |

## 不做事项

- 不修改 `InteractionControlHandler` 的业务方法和公开 API。
- 不新增 SSE 或 long polling。
- 不支持 JSON-RPC batch。
- 不新增 HTTP/WS extension conformance runner。
- 不新增 CLI server 或配置文件格式。
- 不支持浏览器 query token / subprotocol token；v1 只支持 `Authorization: Bearer <token>` header。
- 不在 transport 层做 TLS；生产部署由宿主或反向代理提供 TLS。
