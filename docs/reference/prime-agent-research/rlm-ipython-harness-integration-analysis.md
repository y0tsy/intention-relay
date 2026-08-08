# Интеграция RLM, IPython и Continual Harness в Intention Relay

## Краткий ответ

Да, теоретически в `/home/data/intention-relay` можно встроить:

1. RLM-подобную рекурсивную модель работы;
2. постоянный IPython kernel;
3. Continual Harness из Prime Agent.

Но подключать Prime Agent напрямую как библиотеку к Rust-проекту не стоит.

Правильная архитектура выглядит так:

```text
Intention Relay daemon
        │
        ├── Rust-owned session/run lifecycle
        ├── Rust-owned SQLite persistence
        ├── Rust-owned provider streams
        ├── Rust-owned tool/policy execution
        │
        └── Python kernel sidecar
                ├── persistent IPython state
                ├── rlm(...) bridge
                ├── Python skills
                ├── continual harness client
                └── optional MCP integrations
```

Prime Agent следует использовать как архитектурный источник идей и, возможно, как источник отдельных Python runtime-компонентов. Но не следует запускать рядом вторую систему, которая параллельно владеет сессиями, моделями, persistence, дочерними агентами и daemon lifecycle.

Главный вывод:

```text
RLM: высокая архитектурная совместимость.
IPython: совместимость высокая, но интеграция технически тяжёлая.
Continual Harness: высокая концептуальная совместимость.
Прямой импорт Prime Agent runtime: не рекомендуется.
Python sidecar с typed host bridge: рекомендуемый вариант.
Полноценный production RLM: не маленькая M4-добавка, а отдельная cross-cutting capability.
```

---

# 1. Что сейчас представляет собой Intention Relay

`intention-relay` — локальная single-user Rust-платформа для daemon-owned coding agent.

Главные принципы проекта:

- Rust workspace из небольших crates;
- один локальный daemon владеет runtime;
- Tauri, TUI и REPL являются adapters;
- все границы проходят через typed DTO;
- SQLite является источником durable state;
- на одну session допускается не более одной активной run;
- дополнительные user turns идут в durable queue;
- model providers подключаются через provider-neutral contract;
- state changes сначала коммитятся, затем публикуются;
- runtime не должен автоматически возобновлять внешнюю работу после daemon restart;
- v1 — trusted local workspace, а не sandbox;
- tools, WorkspaceRoot, hooks, Plan/Build, VFR и Headroom являются отдельными подсистемами.

Текущая ветка проекта:

```text
feat/m4-model-streaming
```

Рабочее дерево уже содержит незакоммиченные изменения M4. Во время исследования они не изменялись.

По структуре и исходникам видно, что проект уже прошёл следующие стадии:

```text
M1  DTO/config/workspace skeleton
M2  local protocol/client/daemon foundation
M3  SQLite sessions/events/snapshots/queue
M4  model contracts/providers/streaming run foundation
```

В текущей ветке уже присутствуют:

- provider-neutral `ModelRequestDto`;
- `ModelEventDto`;
- `ModelExecutionDriver`;
- OpenRouter driver;
- Generic Chat Completion driver;
- durable model facts;
- run cursor;
- run replay;
- retry;
- timeout;
- cancellation;
- provider stream lifecycle validation;
- daemon-owned Tokio runtime;
- run-stream subscriptions;
- subscriber queues;
- post-commit publication;
- full durable model context reconstruction;
- scheduling starting runs.

Но ряд будущих crates пока остаётся compile-only skeleton:

```text
intention-tools
intention-workspace
intention-hooks
intention-vfr
intention-headroom
intention-plans
```

Это принципиально важно: полноценный coding-agent RLM требует tool loop и authoritative tool policy, а эти части ещё не активированы.

---

# 2. Почему архитектуры совместимы

Архитектуры Prime Agent и Intention Relay имеют существенное пересечение.

## 2.1. Обе системы разделяют модель и runtime

В Prime Agent поток выглядит примерно так:

```text
LLM provider
    ↓
AgentSession
    ↓
IPython/kernel
    ↓
tools / skills / subagents
```

В Intention Relay:

```text
Provider driver
    ↓
ModelRunExecutionService
    ↓
daemon runtime
    ↓
tools / hooks / workspace / plan policies
```

В обоих случаях provider не должен владеть всей агентной системой.

В Intention Relay provider crate выдаёт только provider-neutral facts:

```text
ModelEventDto
```

Например:

```text
Started
TextDelta
ReasoningDelta
ToolCall
Usage
Finished
```

А runtime занимается:

- lifecycle;
- timeout;
- retry;
- cancellation;
- persistence;
- durable cursor;
- tool-call policy;
- terminal transitions.

Это хорошая основа для RLM: RLM может быть добавлен как orchestration capability, не превращаясь в provider-specific feature.

## 2.2. Обе системы рассматривают session как durable сущность

Prime Agent хранит:

- transcript;
- session tree;
- compaction records;
- goals;
- harness;
- children;
- kernel snapshots.

Intention Relay хранит:

- projects;
- sessions;
- turns;
- runs;
- queue;
- domain events;
- session snapshots;
- run snapshots;
- config revisions;
- model facts;
- run cursors.

Принципиальная совместимость:

```text
Prime Agent session artifacts
≈
Intention Relay durable session/run model
```

Но структуры не одинаковы. Нельзя просто начать писать Prime Agent JSONL в SQLite без адаптации и без определения authoritative model.

## 2.3. Обе системы используют явные lifecycle states

Prime Agent имеет сложный runtime lifecycle вокруг workers, children, kernel и daemon.

Intention Relay явно моделирует:

```text
Queued
Starting
Running
WaitingInput
Completing
Cancelling
Completed
Cancelled
Failed
Interrupted
```

Это даже более формально, чем в большинстве агентных runtime.

Для RLM это означает, что child agent должен быть не просто `tokio::spawn`, а отдельной durable runtime entity с понятным lifecycle.

## 2.4. Обе системы используют post-commit publication

В Intention Relay действует строгое правило:

```text
transaction commit
    ↓
independent durable reread
    ↓
publish live event
```

В Prime Agent child messages, session events и daemon events также проходят через host/runtime ownership.

Это позволяет корректно интегрировать Python kernel, если kernel будет считаться внешним execution participant, а не владельцем истины.

---

# 3. Где естественно встроить RLM

Самое естественное место — между `ModelRunExecutionService` и tool/runtime layer.

Сейчас поток Intention Relay выглядит примерно так:

```text
SendUserTurn
    ↓
accept_user_turn
    ↓
Starting run
    ↓
load_starting_run_model_context
    ↓
ModelRequestDto
    ↓
ModelExecutionDriver
    ↓
ModelEventDto stream
    ↓
durable model facts
    ↓
Completed / Failed
```

RLM добавил бы второй execution path:

```text
ModelRequestDto
    ↓
ModelExecutionDriver
    ↓
ModelEventDto::ToolCall
    ↓
typed tool invocation
    ↓
tool result
    ↓
next model request
```

Если tool call является RLM-вызовом:

```text
ModelEventDto::ToolCall("rlm")
    ↓
RLM host handler
    ↓
create child session/run
    ↓
child executes independently
    ↓
child result through typed message/artifact
    ↓
parent receives child result
```

Критически важно:

```text
RLM ≠ provider
RLM = orchestration capability
```

Не следует встраивать RLM в OpenRouter или Generic Chat driver. Provider должен только нормализовать native stream в `ModelEventDto`.

---

# 4. Соответствие компонентов Prime Agent и Intention Relay

Теоретическое соответствие выглядит так:

| Prime Agent | Intention Relay |
|---|---|
| `AgentSession` | `Session` + active `Run` |
| agent loop | `intention-runtime` или отдельный future agent-loop crate |
| provider stream | `ModelExecutionDriver` |
| tool execution | будущий `intention-tools` |
| workspace enforcement | будущий `intention-workspace` |
| hook dispatcher | будущий `intention-hooks` |
| daemon worker | `intention-daemon` |
| local protocol | `intention-protocol` + `intention-transport` |
| session persistence | `intention-storage` + SQLite |
| subagent registry | будущая domain/storage capability |
| `rlm(...)` Python shim | Python kernel bridge + typed host requests |
| IPython kernel | отдельный Python sidecar/kernel |
| Python skills | Python package discovery/runtime |
| MCP | Python-backed MCP skill layer |
| Continual Harness | local/global durable harness store |
| compaction | future runtime/session context service |
| goal | future typed domain/runtime feature |
| autonomous mode | runtime policy over continuation/gates |

Это показывает, что интеграция возможна, но она затрагивает не один crate.

---

# 5. Что конкретно означает интегрировать IPython

IPython в Prime Agent — не просто запуск `python -c`.

Prime Agent использует:

- отдельный Python kernel process;
- Jupyter protocol;
- ZeroMQ;
- shell channel;
- IOPub channel;
- control channel;
- HMAC message signing;
- persistent namespace;
- snapshot/restore;
- host-request comm;
- async bridge из Python в TypeScript host.

В Intention Relay сейчас нет Python dependency и нет kernel abstraction.

Прямой перенос потребовал бы решить цепочку:

```text
Rust daemon
    ↔
Python process
    ↔
IPython kernel
```

Возможные варианты транспорта:

1. реализовать Jupyter protocol/ZeroMQ непосредственно в Rust;
2. отдельный Python bridge process;
3. JSON-RPC или typed local socket между Rust и Python;
4. stdin/stdout framed protocol;
5. embedded Python через PyO3;
6. Python sidecar, который самостоятельно говорит с IPython.

Наиболее разумен вариант с Python sidecar:

```text
Intention Relay daemon
    ↔ local private typed sidecar protocol
Python sidecar
    ↔ Jupyter protocol
IPython kernel
```

Rust не должен самостоятельно реализовывать весь Jupyter protocol, если для этого нет отдельной необходимости.

---

# 6. Почему не следует сразу использовать PyO3

PyO3 технически возможен, но для текущей архитектуры создаёт значительные риски.

## 6.1. Python runtime внутри daemon

Если встроить CPython непосредственно в Rust daemon:

- daemon process становится зависим от Python runtime;
- усложняется cross-platform packaging;
- усложняется Windows build;
- появляются GIL и lifetime issues;
- async Rust/Tokio и Python asyncio требуют отдельной интеграции;
- падение Python extension может повлиять на daemon;
- kernel restart становится менее изолированным;
- Python packages будут устанавливаться в окружение daemon;
- FFI boundary становится частью основного daemon lifecycle.

Это плохо сочетается с текущим принципом:

> daemon владеет runtime, но implementation resources не пересекают DTO boundaries.

## 6.2. Проблемы с trusted execution

Intention Relay уже прямо говорит:

```text
v1 is trusted local execution, not a sandbox
```

Встроенный Python сделает execution surface ещё шире и плотнее свяжет его с daemon.

## 6.3. Sidecar лучше соответствует Prime Agent

Сам Prime Agent отделяет TypeScript host от IPython kernel. Поэтому для Intention Relay естественнее:

```text
Rust host
    ↔
Python sidecar
    ↔
IPython
```

а не:

```text
Rust daemon embeds Python
```

Sidecar также позволяет независимо:

- перезапускать kernel;
- ограничивать ресурсы;
- обнаруживать падение Python;
- менять Python environment;
- поддерживать Windows и Unix отдельными адаптерами;
- отделить Python failure от daemon failure.

---

# 7. Рекомендуемая IPython-архитектура

## 7.1. Rust side

Нужен отдельный capability/service, условно:

```text
intention-kernel
```

или:

```text
intention-python-runtime
```

Его ответственность должна быть заранее объявлена в `quality/architecture.toml`.

Он может владеть:

- kernel process lifecycle;
- sidecar process lifecycle;
- session-to-kernel mapping;
- startup/shutdown;
- execution request IDs;
- cancellation;
- output limits;
- kernel restart;
- state snapshot metadata;
- typed host request dispatch;
- Python skill installation status.

Публичная граница должна быть DTO-only:

```rust
KernelExecutionRequestDto
KernelExecutionResultDto
KernelOutputChunkDto
KernelHostRequestDto
KernelHostResponseDto
KernelStateSnapshotDto
KernelStatusDto
```

Нельзя выставлять наружу:

- `Child`;
- `tokio::process` types;
- `JoinHandle`;
- `UnixStream`;
- `PathBuf` как arbitrary implementation resource;
- Python object;
- raw Jupyter message;
- ZeroMQ socket.

## 7.2. Python side

Python sidecar может использовать:

- `ipykernel`;
- `jupyter_client`;
- минимальный package по образцу `prime-agent-runtime`;
- `rlm` object;
- Python-backed skills;
- `harness` object.

Python bridge должен быть адаптирован к Rust DTO contract.

Пример Python host request:

```python
await host_request(
    "rlm.run",
    {
        "parent_session_id": "...",
        "parent_run_id": "...",
        "prompt": "...",
        "name": "...",
    },
)
```

Rust host отвечает после durable admission:

```json
{
  "status": "ok",
  "child_session_id": "...",
  "child_run_id": "...",
  "child_handle_id": "...",
  "model": "..."
}
```

Python возвращает convenience object:

```python
RLMSpawnHandle(
    child_session_id=...,
    child_run_id=...,
    name=...,
    model=...,
)
```

---

# 8. Главная несовместимость: Prime Agent RLM TypeScript-owned, Intention Relay Rust-owned

В Prime Agent:

```text
Python rlm()
    ↓
TypeScript AgentSession
    ↓
child AgentSession
```

В Intention Relay основная система должна быть Rust-owned:

```text
Python request
    ↓
Rust daemon
    ↓
Rust session/run model
```

Нельзя буквально перенести Python `rlm/__init__.py` и ожидать, что он заработает. Python shim должен быть переписан как thin client к Rust daemon.

Сравнение:

```text
Prime Agent:
  Python shim → TypeScript host

Intention Relay:
  Python shim → Rust host
```

Все authoritative state должно находиться в Rust:

- child identity;
- parent relationship;
- status;
- session/run IDs;
- model selection;
- lifecycle;
- cancellation;
- usage;
- messages;
- persistence;
- recovery;
- access policy.

Python должен держать только:

- convenience objects;
- local variables;
- helper functions;
- short-lived task handles;
- skill wrappers;
- orchestration state, который можно восстановить или потерять без corruption durable state.

---

# 9. Как встроить RLM в текущий one-run invariant

В Intention Relay уже есть важное правило:

> одна активная run на session.

Это не мешает RLM, но требует правильного моделирования.

## Вариант A: child — отдельная session

Рекомендуемый вариант:

```text
parent session
  └── parent run
        ├── child session 1
        │     └── child run
        ├── child session 2
        │     └── child run
        └── child session 3
              └── child run
```

Тогда invariant сохраняется:

```text
каждая session имеет не более одной active run
```

Parent может:

- spawn children;
- продолжать собственную работу;
- получать messages;
- читать child artifacts;
- получать child terminal events;
- отправлять follow-up;
- отменять child.

Это наиболее близко к Prime Agent.

## Вариант B: child — под-run внутри parent session

```text
one session
  ├── parent run
  ├── child run
  └── child run
```

Этот вариант хуже, потому что нарушает текущий invariant и усложняет:

- event ordering;
- queue;
- active-run constraints;
- replay;
- config snapshots;
- cancellation;
- UI representation;
- session-scoped subscription.

## Вариант C: child — ephemeral task без durable run

Это проще для прототипа, но несовместимо с сильными сторонами Intention Relay:

- нет recovery;
- нет durable status;
- нет proper cancellation;
- нет usage attribution;
- нет replay;
- daemon restart теряет состояние;
- parent-child relation не имеет durable evidence.

Для production-архитектуры этот вариант не рекомендуется.

---

# 10. Предлагаемая domain model для RLM

Нужны новые typed entities.

Минимально:

```text
AgentTreeId
AgentNodeId
ChildAgentId
ParentRunId
ParentSessionId
ChildSessionId
ChildRunId
AgentMessageId
AgentNameDto
```

Вместо строки `parent_id` лучше использовать отдельную typed relationship DTO:

```rust
AgentParentLinkDto {
    parent_session_id: SessionId,
    parent_run_id: RunId,
    child_session_id: SessionId,
    child_run_id: RunId,
}
```

Для registry:

```rust
RlmChildProjectionDto {
    child_id: ChildAgentId,
    parent_session_id: SessionId,
    parent_run_id: RunId,
    child_session_id: SessionId,
    child_run_id: RunId,
    name: AgentNameDto,
    status: ChildStatusDto,
    model: ModelSelectionDto,
    session_artifact_ref: ArtifactReferenceDto,
    created_at: TimestampDto,
    completed_at: Option<TimestampDto>,
}
```

Для admission:

```rust
SpawnRlmChildCommandDto {
    parent_session_id: SessionId,
    parent_run_id: RunId,
    child_session_id: SessionId,
    child_run_id: ChildRunId,
    prompt: String,
    requested_name: Option<AgentNameDto>,
    model: Option<ModelSelectionDto>,
}
```

Для result delivery:

```rust
AgentMessageDto {
    message_id: AgentMessageId,
    sender: AgentNodeId,
    receiver: AgentNodeId,
    parent_run_id: Option<RunId>,
    child_run_id: Option<RunId>,
    content: String,
    delivery_status: DeliveryStatusDto,
    occurred_at: TimestampDto,
}
```

Raw Python dictionaries не должны становиться domain contract.

---

# 11. RLM host-request flow в Intention Relay

Пример:

```python
handle = await rlm(
    "Review the authentication code",
    name="auth-reviewer",
)
```

Поток должен быть таким:

```text
IPython
  ↓
Python rlm shim
  ↓
Kernel host-request bridge
  ↓
Rust kernel service
  ↓
Daemon application command
  ↓
SpawnRlmChildCommandDto
  ↓
SQLite transaction:
  - child session
  - child run
  - parent-child relationship
  - child admission event
  ↓
daemon scheduler
  ↓
child run actor
  ↓
provider stream
```

Ответ родителю должен приходить только после durable admission:

```text
child record committed
child run committed
parent registry committed
      ↓
RlmSpawnHandle returned
```

Как и в Prime Agent, `rlm()` не должен ждать ответа child.

Второй этап — получение результата:

```text
child run
  ↓
AgentMessageSent / ArtifactCreated
  ↓
durable commit
  ↓
parent session event
  ↓
parent model context update
```

---

# 12. Как child должен возвращать результат

В Prime Agent есть два основных пути:

1. `agent_message.send(...)`;
2. запись результата в файл.

В Intention Relay лучше поддержать оба, но с разным статусом.

## 12.1. Typed agent message

Основной путь:

```python
await agent_message.send(
    "Authentication review complete: ...",
    receiver_role="parent",
)
```

Rust host создаёт durable events:

```text
AgentMessageSent
AgentMessageDelivered
```

или:

```text
AgentMessageQueued
```

Parent получает typed run/session event.

Необходимо ограничивать:

- размер сообщения;
- количество pending messages;
- rate limit;
- допустимые relationship targets;
- delivery semantics.

## 12.2. Artifact result

Child может записать report через обычный WorkspaceRoot/tool policy.

Parent получает artifact reference:

```text
ArtifactReferenceDto {
    artifact_id,
    relative_path,
    content_kind,
    size,
    checksum,
}
```

Крупный report не следует передавать через message DTO.

## 12.3. Не передавать полный child transcript

Parent должен получать:

- краткий typed message;
- artifact reference;
- bounded preview;
- explicit child status.

Полный child transcript остаётся в child session.

Это предотвращает разрастание parent context и соответствует parent-scoped registry Prime Agent.

---

# 13. Что уже готово для child agents

## 13.1. Durable session/run hierarchy

Session/run model уже отделяет:

- user turn;
- run;
- queue;
- config snapshot;
- durable events;
- run snapshot.

Это хорошая основа для child sessions.

## 13.2. Exact run-scoped replay

В M4 уже есть:

```text
RunSnapshotDto
RunEventCursorDto
RunEventTailPageDto
RunReplayDto
```

Child agents могут использовать ту же модель replay.

## 13.3. Cancellation signal

В `intention-model` есть provider-neutral:

```rust
ModelCancellationSignal
```

Он не привязан к Tokio и позволяет daemon host управлять cancellation.

Это можно переиспользовать для child run cancellation.

## 13.4. Daemon-owned Tokio runtime

Текущий `intention-daemon` уже создаёт private Tokio runtime и запускает tasks:

```rust
tokio::spawn(async move {
    // child/model work
});
```

Это естественное место для child execution scheduling.

## 13.5. Subscriber isolation

Для run-stream уже есть:

- private queue per subscriber;
- capacity 64;
- write deadline;
- slow-peer resync;
- removal only of slow peer.

Похожая модель может использоваться для child activity updates, но не нужно автоматически превращать каждый child event в глобальный stream.

---

# 14. Что отсутствует и является реальным блокером

## 14.1. Tool runtime пока skeleton

Сейчас:

```text
intention-tools — compile-only skeleton
intention-workspace — compile-only skeleton
intention-hooks — compile-only skeleton
```

Пока не реализованы:

- read;
- search;
- write;
- edit;
- execute;
- WorkspaceRoot resolution;
- path containment;
- hook dispatcher;
- Plan policy.

Полноценный coding-agent RLM нельзя считать production-ready до M5.

Можно сделать kernel-only prototype раньше, но он сможет в основном:

- анализировать данные;
- вызывать внешние Python functions;
- выполнять Python;
- запускать ограниченные sidecar commands.

## 14.2. Tool calls сейчас намеренно deny'ятся

В `ModelRunExecutionService` при `ToolCall` сейчас происходит примерно следующее:

```text
record ToolCall
record tool_execution_unavailable failure
fail run
```

Это принятое M4 решение.

Для RLM потребуется изменить execution loop:

```text
ModelEvent::ToolCall
    ↓
validate against tool registry
    ↓
apply mode/risk/workspace hooks
    ↓
execute tool
    ↓
persist result
    ↓
append tool result to model context
    ↓
next model request
```

Это переход от one-shot streaming run к полноценному agent loop.

## 14.3. ModelRequest пока text-only

Текущая модель поддерживает роли:

```text
System
User
Assistant
```

и текстовый content.

Для RLM/tool calls потребуется полноценная поддержка:

- assistant tool-call message;
- tool result message;
- tool descriptors/schema;
- child result injection;
- tool-call identity;
- possibly images and structured content.

Нужны DTO наподобие:

```text
AssistantToolCallMessageDto
ToolResultMessageDto
ToolDescriptorDto
ToolInvocationDto
ToolResultDto
ModelContextMutationDto
ContinuationReasonDto
```

## 14.4. Нет kernel lifecycle

В текущем Rust workspace нет production abstraction для:

- Python executable discovery;
- virtualenv management;
- IPython startup;
- Jupyter connection management;
- host-request comm;
- snapshot/restore;
- kernel restart;
- per-session kernel ownership;
- output limits;
- Python skill installation.

Это отдельная substantial subsystem.

## 14.5. Нет durable harness

В текущем Rust workspace не видно реализованного harness store.

Нужно добавить:

- local session harness;
- global user harness;
- project scope, если он нужен;
- entries;
- refinement history;
- versioning;
- rollback;
- merge policy;
- prompt projection;
- conflict handling;
- cross-process synchronization.

## 14.6. Нет полноценного prompt/context builder уровня Prime Agent

В Intention Relay уже есть durable context reconstruction:

```text
completed user turns
completed assistant content
current user turn
```

Но это ещё не system prompt/context orchestration.

Для Prime Agent-like behavior потребуются:

- base system prompt;
- project instructions;
- global instructions;
- skills metadata;
- loaded skill content;
- harness overview;
- child-agent guidance;
- current goal;
- tool descriptions;
- mode policy;
- VFR/Headroom instructions;
- date/workspace information.

---

# 15. Continual Harness в Intention Relay

Интеграция Continual Harness теоретически возможна и хорошо соответствует durable SQLite architecture.

Но Python implementation из Prime Agent нельзя просто положить в SQLite-backed Rust систему без изменения ownership.

В Prime Agent harness представляет собой JSON state:

```text
harness_state.json
```

В нём есть:

```text
entries:
  prompt
  memory
  skill
  subagent

refinements:
  refinement events
```

В Intention Relay правильнее сделать Rust-owned durable contract.

## 15.1. Предлагаемая domain model

```text
HarnessScopeDto:
  LocalSession
  GlobalUser
  Project

HarnessKindDto:
  Prompt
  Memory
  Skill
  Subagent

HarnessEntryId
HarnessRevision
HarnessEntryDto
HarnessRefinementEventDto
HarnessChangeDto
```

Пример:

```rust
HarnessEntryDto {
    entry_id: HarnessEntryId,
    scope: HarnessScopeDto,
    kind: HarnessKindDto,
    title: String,
    content: String,
    path: HarnessPathDto,
    reference: Option<SkillReferenceDto>,
    arguments: Option<SkillArgumentsDto>,
    metadata: HarnessMetadataDto,
    source: HarnessEntrySourceDto,
    version: u32,
    created_at: TimestampDto,
    updated_at: TimestampDto,
}
```

## 15.2. Где хранить

Рекомендуемый вариант:

```text
SQLite:
  harness_entries
  harness_entry_revisions
  harness_refinement_events
```

Global entries можно хранить в той же database с явным scope:

```text
scope = global_user
```

либо в отдельной user-level database, если потребуется разделить lifecycle. Но второй database добавляет synchronization complexity.

Поскольку в проекте SQLite уже является authoritative durable store, отдельный `harness_state.json` создавал бы второй source of truth и нарушал бы общую модель.

## 15.3. Что оставить в Python

Python может получить typed snapshot и convenience API:

```python
harness.overview()
harness.get(...)
harness.create_memory(...)
harness.create_skill(...)
harness.create_subagent(...)
```

Но writes должны идти через Rust host:

```text
Python harness API
    ↓
host request
    ↓
Rust application command
    ↓
SQLite transaction
    ↓
typed result
```

Python не должен напрямую писать основной harness JSON, если Intention Relay хочет гарантировать durable transactions и event audit.

---

# 16. Как реализовать `/refine`

Prime Agent `/refine` — это model-backed process:

```text
trajectory
    ↓
review model
    ↓
JSON proposal
    ↓
validate edits
    ↓
apply harness changes
    ↓
record refinement
    ↓
rebuild system prompt
```

Для Intention Relay это нужно разделить на две части.

## 16.1. Rust-owned

Rust должен владеть:

- refinement command;
- trajectory selection;
- current harness state;
- validation;
- scope;
- edit application;
- transaction;
- rollback;
- refinement event;
- audit.

## 16.2. Model-owned

Модель может предложить:

```json
{
  "summary": "...",
  "rationale": "...",
  "expected_outcome": "...",
  "edits": []
}
```

Но это нужно декодировать в typed DTO:

```rust
RefinementProposalDto
RefinementEditDto
```

Плохой вариант:

```rust
serde_json::Value
```

или сохранение необработанного ответа модели.

Хороший вариант:

```text
Model output
    ↓
strict JSON decoder
    ↓
RefinementProposalDto validation
    ↓
scope/policy validation
    ↓
SQLite transaction
```

## 16.3. Immutable base prompt

Base system prompt должен быть immutable.

Harness может добавлять:

- prompt notes;
- memories;
- skills;
- subagent specs.

Но не должен перезаписывать основную архитектурную политику Intention Relay.

Особенно нельзя через harness отключать:

- WorkspaceRoot;
- DTO-first;
- secret redaction;
- daemon authority;
- Plan/Build policy;
- one active run;
- no automatic resume.

---

# 17. Главный риск Continual Harness

Continual Harness может стать конфликтующим вторым policy engine.

В Intention Relay уже есть:

```text
config
 domain invariants
runtime lifecycle
tool hooks
plan policy
workspace policy
```

Если harness начинает добавлять произвольные instructions, возможны конфликты:

```text
Harness says: execute directly
Workspace policy says: reject outside root
```

или:

```text
Harness says: resume after restart
Architecture says: never resume external work
```

Поэтому приоритеты должны быть формально заданы:

```text
1. Rust hard safety/domain invariants
2. User explicit command
3. Session/run policy
4. Project instructions
5. Global harness
6. Local harness
7. Model-generated proposal
```

Harness не должен иметь возможности обходить первые три уровня.

Важно разделить:

```text
instructions / memories
```

и:

```text
enforced policy
```

Continual Harness — context/prompt layer, а не security layer.

---

# 18. IPython и WorkspaceRoot

Это одна из самых опасных точек интеграции.

Prime Agent запускает Python и shell с правами пользователя. В Intention Relay WorkspaceRoot должен быть обязательным для filesystem tools.

Но если IPython получает прямой shell:

```python
%%bash
rm -rf ...
```

то WorkspaceRoot tool hook этого не контролирует.

Текущая архитектура Intention Relay сама предупреждает:

> execute cannot be fully constrained

Для Plan mode отдельно указывается, что shell не является техническим sandbox.

Поэтому есть три режима IPython.

## Режим 1: unrestricted trusted kernel

```text
IPython может читать, писать и исполнять всё с правами пользователя
```

Плюсы:

- ближе к Prime Agent;
- проще;
- максимальная гибкость.

Минусы:

- bypass WorkspaceRoot;
- bypass tool hooks;
- bypass Plan policy;
- bypass confirmation;
- bypass audit;
- dangerous for project instructions and skills.

Это можно разрешить только как явно выставленный trusted mode.

## Режим 2: restricted host bridge

Python не получает прямой shell-доступ. Он вызывает:

```python
await host.read(...)
await host.search(...)
await host.edit(...)
await host.execute(...)
```

Rust host применяет:

- WorkspaceRoot;
- mode;
- hook chain;
- risk policy;
- persistence;
- event publication.

Это лучше соответствует Intention Relay.

Но это уже не полностью свободный IPython. Это Python orchestration layer над typed host capabilities.

## Режим 3: external sandbox

Python kernel и shell запускаются во внешнем sandbox:

- container;
- отдельный user;
- OS sandbox;
- worktree;
- restricted namespace.

Это безопаснее, но sandbox/worktree isolation прямо отложена текущей v1-архитектурой.

## Рекомендация

Для production Intention Relay:

```text
IPython = orchestration/control plane
Rust host tools = authoritative capability plane
```

Python может:

- анализировать данные;
- писать helper functions;
- агрегировать результаты;
- запускать fan-out;
- вызывать skills;
- вызывать RLM;
- ждать messages.

Но файловые и process capabilities проходят через typed host requests.

---

# 19. Как сохранить context при IPython

Prime Agent разделяет:

```text
conversation context
Python kernel namespace
```

Intention Relay уже сохраняет durable model context, но после добавления IPython появятся ещё два вида состояния:

```text
1. durable model transcript
2. live Python namespace
```

Нужно явно решить, что происходит при:

- daemon restart;
- kernel crash;
- run cancellation;
- compaction;
- child completion;
- session fork;
- configuration revision change.

Рекомендуемый подход:

```text
Transcript is authoritative durable state.
Python namespace is recoverable convenience state.
```

При kernel restart:

- transcript сохраняется;
- active kernel namespace может быть потерян;
- model получает `KernelRestarted` notice;
- model получает инструкцию восстановить нужные variables;
- при необходимости kernel snapshot восстанавливает сериализуемые values.

Нельзя считать Python namespace единственным источником прогресса.

Это согласуется с текущей Intention Relay policy:

> daemon restart does not automatically resume external work.

---

# 20. Compaction и Headroom

В Prime Agent есть две разные идеи:

1. compaction — сжатие старой conversation history;
2. Headroom/CCR — compression/retrieval tool output.

В Intention Relay это хорошо ложится на уже запланированные подсистемы:

- Headroom предусмотрен отдельным crate;
- VFR предусмотрен отдельным crate;
- durable events и snapshots отделены от model context.

## 20.1. Compaction

Нужен отдельный service, условно:

```text
intention-context
```

или отдельный слой в runtime/application.

Он должен:

- выбрать messages для summary;
- вызвать model summarizer;
- записать `ContextCompacted` event;
- сохранить summary;
- сохранить file-operation references;
- построить новый model context;
- не удалять оригинальные durable facts.

Модель хранения:

```text
raw durable facts remain
model-visible context uses summary + recent entries
```

## 20.2. Headroom

Headroom должен работать через существующий typed hook pipeline:

```text
physical tool output
    ↓
normalized result
    ↓
persist
    ↓
Headroom compresses model context
    ↓
model gets compressed representation
```

Это уже заложено архитектурой Intention Relay и практически совпадает с Prime Agent design.

## 20.3. VFR

VFR также почти напрямую совместим:

```text
read physical file
    ↓
VFR structured representation
    ↓
model sees virtual view
    ↓
expand/raw-read on demand
```

Но сейчас `intention-vfr` skeleton, поэтому это будущая M8 work.

---

# 21. MCP integration

MCP в Prime Agent реализован как Python-backed skill.

Для Intention Relay возможна такая схема:

```text
IPython skill
    ↓
MCP HTTP endpoint
```

Но нужно решить, где находятся credentials.

У Intention Relay уже есть строгая credential policy:

- credentials могут находиться в TOML startup material;
- credentials не должны попадать в DTO;
- credentials не должны попадать в events/snapshots/logs;
- provider SDK resources скрыты;
- configuration snapshots credential-free.

Для MCP лучше:

```text
Rust host:
  - owns credentials and OAuth tokens
  - exposes only typed mcp host requests

Python:
  - exposes friendly async skill API
  - never persists credentials itself
```

Но MCP tools могут не только читать данные. Они могут:

- изменять Linear;
- создавать Notion pages;
- отправлять внешние запросы;
- запускать side effects.

Поэтому их нужно включить в tool/risk policy, а не разрешать как необозначенные Python calls.

Для безопасной первой версии целесообразно поддержать только HTTP MCP endpoints и явно классифицировать mutating operations.

---

# 22. Как должен выглядеть полноценный agent loop

Сейчас Intention Relay M4 реализует streaming one-run path.

Для Prime Agent-like functionality потребуется цикл более высокого уровня:

```text
run starts
    ↓
build model context
    ↓
call provider
    ↓
receive stream facts
    ├── text
    ├── reasoning
    ├── usage
    ├── tool call
    └── finished
    ↓
if tool call:
    validate tool
    apply policy/hooks
    execute tool
    persist tool result
    append tool result to context
    ↓
call provider again
    ↓
repeat until terminal
```

Model request должен уметь включать:

```text
system
user
assistant text
assistant tool call
tool result
```

Текущий `ModelMessageDto` имеет только:

```text
System
User
Assistant
```

и text content. Для полноценного loop этого недостаточно.

Дополнительно понадобится отделить:

```text
provider streaming facts
```

от:

```text
agent-loop context messages
```

Provider event `ToolCall` — ещё не выполненный tool. Runtime должен:

1. валидировать tool identity;
2. проверить schema;
3. проверить policy;
4. выполнить tool;
5. сохранить result;
6. добавить tool result в следующую model request;
7. продолжить цикл.

---

# 23. Где должен жить agent loop

Есть два основных варианта.

## Вариант A: расширить `intention-runtime`

Плюсы:

- lifecycle уже находится там;
- runtime уже знает cancellation/retry/terminal state;
- provider-neutral boundary уже существует;
- проще сохранять current state.

Минусы:

- runtime станет значительно сложнее;
- нужно отделить pure lifecycle decisions от async actor logic;
- появится опасность большого monolithic crate;
- tool, kernel, context и child orchestration могут смешаться с state machine.

## Вариант B: отдельный `intention-agent-loop`

Например:

```text
intention-agent-loop
```

Он может владеть:

- model → tool → model cycle;
- context assembly;
- tool invocation;
- continuation;
- RLM host calls;
- child result fan-in;
- model-facing tool schema.

А `intention-runtime` продолжит владеть:

- state transitions;
- cancellation policy;
- durable lifecycle;
- run outcome.

Это архитектурно чище.

Но новый production crate обязан быть заранее объявлен в:

```text
quality/architecture.toml
```

и получить:

- responsibility;
- test target;
- coverage tier;
- dependency policy.

## Рекомендация

Для полноценного RLM лучше отдельный crate, а не превращение `intention-runtime` в большой monolith.

Возможная зависимость:

```text
intention-agent-loop
    depends on:
      intention-runtime
      intention-model
      intention-tools
      intention-hooks
      intention-domain
      intention-storage contracts
      intention-kernel contract
```

Но конкретное направление нужно будет проверить против approved crate graph, чтобы не создать cycle.

---

# 24. Parent/child continuity

Parent должен иметь возможность:

```text
spawn child
continue own work
receive child message
inspect child
send follow-up
delete/cancel child
```

Нужны durable events:

```text
ChildAgentAdmitted
ChildAgentStarted
ChildAgentMessageSent
ChildAgentMessageDelivered
ChildAgentCompleted
ChildAgentFailed
ChildAgentCancelled
ChildAgentDeleted
```

Не стоит помещать весь child state в parent session snapshot.

Лучше:

```text
parent session snapshot:
  child registry summaries

child session snapshot:
  full child projection
```

Parent snapshot содержит bounded child summaries:

```text
child id
name
status
latest cursor
latest safe preview
```

Full transcript остаётся child-owned.

Это предотвращает разрастание parent context и соответствует parent-scoped registry Prime Agent.

---

# 25. Usage и cost accounting

Prime Agent отдельно учитывает usage child agents.

В Intention Relay уже есть durable `UsageDto` и model facts.

Нужно добавить:

```text
ChildUsageAttributionDto
```

или domain event:

```text
ChildUsageAttributed
```

Рекомендуемая модель:

```text
child run:
  own usage

parent run:
  own usage
  attributed child usage

agent tree:
  root aggregate
  per-node own usage
  per-node attributed usage
```

Нельзя считать child usage дважды.

Например:

```text
root aggregate = root own + all child own
parent own = parent model requests only
child own = child model requests only
```

Это нужно зафиксировать contract tests, иначе UI и analytics будут противоречивыми.

---

# 26. Model selection для детей

Prime Agent позволяет:

```python
await rlm.find_models(...)
await rlm("task", model="provider/model")
```

В Intention Relay run получает immutable `ConfigSnapshotDto`.

Для children нужно решить:

1. наследовать родительский snapshot;
2. выбрать другую модель из разрешённого catalog;
3. получить отдельный config revision;
4. запретить другую модель.

Рекомендуемая первая версия:

```text
child inherits parent provider/model/config snapshot
```

Позже:

```text
requested child model
    ↓
validate against daemon model catalog and policy
    ↓
create child with explicit immutable config snapshot
```

Нельзя позволить Python произвольно передать provider credential или endpoint.

При выборе другой модели child должен получить только safe selection DTO. Credentials остаются в Rust-owned startup/provider material.

---

# 27. Restart and recovery semantics

Prime Agent может восстанавливать retained children.

Intention Relay имеет более строгую политику:

```text
unfinished run → Interrupted
external work never resumes automatically
```

Это важное расхождение.

При добавлении RLM нужно определить:

- parent restart;
- child restart;
- kernel restart;
- child admission committed but process not started;
- child process started but no first event;
- child result sent but parent disconnected;
- parent closes while child is running;
- child completion after parent session terminal.

Рекомендуемая v1 policy:

```text
Rust durable child run status is authoritative.
After daemon restart:
  - unfinished parent/child external runs become Interrupted;
  - no model/provider/tool execution resumes automatically;
  - retained completed child transcripts remain readable;
  - new explicit follow-up can start a new run.
```

Это менее автономно, чем Prime Agent, но полностью соответствует Intention Relay charter.

Позже можно добавить explicit resume workflow:

```text
ResumeChildCommandDto
```

с явным пользовательским подтверждением и новым run.

---

# 28. Почему не стоит переносить daemon из Prime Agent

Prime Agent daemon и Intention Relay daemon решают похожие задачи, но имеют разные contracts.

Prime Agent:

- JS/TS;
- JSONL/local sockets;
- session workers;
- flexible extension model;
- runtime resources в host;
- application-level structures.

Intention Relay:

- Rust;
- DTO-first;
- strict architecture checks;
- SQLite transaction authority;
- explicit schema/version policy;
- strong crate ownership;
- cross-platform local sockets/named pipes;
- mandatory outcome tests.

Лучше перенести идеи:

- worker lifecycle;
- child registry;
- host-request bridge;
- persistent kernel;
- harness;
- compaction;
- message delivery;
- context tree.

Но не копировать Prime Agent daemon code или persistence model напрямую.

---

# 29. Самое важное архитектурное решение: кем является IPython

Нужно выбрать роль IPython в Intention Relay.

## Модель 1: IPython как обычный tool

```text
ModelToolCall("ipython")
    ↓
execute arbitrary Python
    ↓
result
```

Это самый простой путь.

Но тогда:

- Python state не является полноценной session state;
- RLM host requests нужно отдельно прокидывать;
- file/tool policy легко обходится;
- daemon не понимает внутренние actions;
- child spawn может быть не полностью durable;
- audit неполный.

## Модель 2: IPython как control plane

```text
Model invokes IPython
    ↓
Python program orchestrates work
    ↓
typed host requests perform authoritative actions
```

Это настоящий RLM.

Python может:

- держать variables;
- вызывать skills;
- делать анализ;
- запускать fan-out;
- ждать messages;
- вызывать host functions.

Rust остаётся владельцем:

- tools;
- state;
- sessions;
- children;
- policies;
- persistence;
- providers;
- daemon.

Это рекомендуемый вариант.

## Модель 3: Python как внешний full agent

```text
Rust daemon starts a complete Prime Agent subprocess
```

Это проще всего для первоначального эксперимента, но создаёт две конкурирующие системы:

```text
Intention Relay runtime
Prime Agent runtime
```

И появляются два источника истины для:

- sessions;
- models;
- tools;
- prompts;
- child agents;
- persistence;
- cancellation;
- telemetry;
- config.

Для production это не рекомендуется.

---

# 30. Практический интеграционный план

## Этап 0. Decision document, без изменения production code

Сначала нужно зафиксировать архитектурное решение:

```text
RLM/IPython/Continual Harness Integration Decision
```

В нём определить:

- Python sidecar vs PyO3;
- IPython ownership;
- Rust host authority;
- child-as-session policy;
- restart semantics;
- security mode;
- harness scopes;
- DTO families;
- new crates;
- milestone placement;
- quality-gate additions;
- migration/compatibility policy.

Сейчас это не обычная M4 lane feature. По текущему `m4.md` RLM/IPython прямо не входит в approved M4 scope.

В M4 charter явно отложены:

- M5 tools;
- WorkspaceRoot;
- hooks;
- Plan/Build;
- M6 UI;
- config live reload;
- automatic external work resumption.

Поэтому полноценный RLM должен быть отдельным roadmap milestone или отдельным decision/change request.

## Этап 1. Kernel proof of concept

Не подключать сразу все tools.

Сделать минимальный sidecar:

```text
Rust daemon
  ↔ Unix socket/named pipe
Python sidecar
  ↔ IPython kernel
```

Проверить:

- launch;
- hello;
- execute;
- stdout/stderr;
- cancellation;
- kernel crash;
- bounded output;
- restart;
- per-session ownership;
- no secret leakage.

На этом этапе можно реализовать:

```python
x = 1
x += 1
```

и:

```python
await host_request("health")
```

Но не разрешать unrestricted production file mutation.

## Этап 2. Typed host bridge

Добавить DTO:

```text
KernelExecutionRequestDto
KernelExecutionResultDto
KernelOutputChunkDto
HostRequestDto
HostResponseDto
```

Проверить:

- correlation IDs;
- session/run identity;
- cancellation;
- error mapping;
- protocol compatibility;
- output limits;
- no raw Python/Jupyter types across public boundaries.

## Этап 3. Python state lifecycle

Определить:

- kernel per session или per run;
- snapshot/restore;
- kernel namespace size limit;
- serialization failures;
- restart notice;
- relation to durable transcript.

Рекомендуется:

```text
one kernel per active session
```

а не один kernel per run, потому что RLM предполагает state across turns.

Kernel process должен принадлежать daemon session actor и закрываться при session shutdown.

## Этап 4. Agent loop tool protocol

Расширить model DTO:

```text
assistant tool calls
tool results
tool descriptors
```

Добавить `intention-agent-loop`.

Сначала поддержать один tool:

```text
ipython
```

Потом host bridge functions и остальные tools.

## Этап 5. Workspace-aware host capabilities

До unrestricted shell нужно реализовать M5:

- `intention-tools`;
- `intention-workspace`;
- `intention-hooks`;
- WorkspaceRoot;
- execute policy;
- Plan/Build policy.

Для RLM безопасный вариант:

```python
await host.read(...)
await host.search(...)
await host.edit(...)
await host.execute(...)
```

вместо прямого:

```python
Path(...).write_text(...)
subprocess.run(...)
```

## Этап 6. Child sessions / RLM

Добавить:

- child session/run entities;
- parent-child relation;
- admission transaction;
- child registry;
- message DTO;
- child status;
- cancellation;
- usage attribution;
- child replay;
- child observe;
- follow-up;
- terminal result delivery.

## Этап 7. Continual Harness

Добавить Rust-owned SQLite tables and DTOs:

```text
harness_entries
harness_entry_revisions
harness_refinement_events
```

Затем Python API:

```python
rlm.harness.create_memory(...)
rlm.harness.list(...)
rlm.harness.overview()
```

Все writes проходят host bridge.

## Этап 8. Compaction and Headroom

После основного agent loop:

- context compaction;
- harness prompt projection;
- Headroom/CCR;
- VFR;
- retrieve/expand;
- context rebuild tests.

## Этап 9. MCP

Только после typed tool/risk/policy layer.

MCP должен стать skill adapter, но всё равно проходить:

- credential boundary;
- action classification;
- persistence;
- audit;
- cancellation;
- external side-effect policy.

## Этап 10. UI

Только после daemon contracts:

- child tree;
- Python/kernel status;
- tool activity;
- harness status;
- compaction status;
- parent/child messages;
- reconnect/replay.

---

# 31. Что можно сделать быстрее всего

Если нужен не production feature, а теоретический прототип, минимальная версия:

```text
1. Add intention-kernel sidecar crate
2. Spawn managed Python process
3. Execute IPython cells through local private protocol
4. Expose one host_request("rlm.run")
5. Create child in memory
6. Return child handle
7. Persist only an audit event
```

Но это будет:

```text
RLM proof of concept
```

а не полноценная интеграция.

Для production minimum нужны:

```text
- typed kernel protocol
- durable child session/run
- tool loop
- WorkspaceRoot enforcement
- cancellation
- child replay
- message delivery
- restart semantics
- harness persistence
- quality-gate additions
```

---

# 32. Теоретическая оценка совместимости

## RLM

**Совместимость: высокая.**

Причины:

- current model/runtime separation подходит;
- daemon уже владеет execution;
- durable run hierarchy подходит для child sessions;
- run-scoped replay уже существует;
- cancellation signal уже provider-neutral;
- Tokio host уже есть.

Главная работа:

- полноценный tool loop;
- child session model;
- host request bridge;
- tool policy.

## IPython

**Совместимость: средняя/высокая, но интеграционно тяжёлая.**

Причины:

- IPython можно запустить sidecar;
- local daemon transport уже есть;
- async Rust foundation уже есть;
- typed DTO rules подходят.

Основные сложности:

- Python process lifecycle;
- Jupyter protocol;
- cross-platform packaging;
- namespace snapshot;
- security;
- WorkspaceRoot bypass;
- Rust/Python async bridge.

## Continual Harness

**Совместимость: высокая концептуально, но хранить нужно по-Rust-овски.**

Причины:

- SQLite уже authoritative;
- durable event model уже существует;
- config/session scopes уже есть;
- DTO-first policy подходит.

Основная работа:

- typed schema;
- scope/merge rules;
- refinement protocol;
- rollback;
- prompt projection;
- protection from policy override.

---

# 33. Главные риски интеграции

## Риск 1. Два источника истины

Если Python начинает напрямую писать:

- session files;
- harness JSON;
- child registry;
- tool results;

то Rust database и Python files расходятся.

Решение:

```text
Rust authoritative
Python convenience/cache
```

## Риск 2. Обход WorkspaceRoot

Свободный IPython shell может обойти:

- path policy;
- Plan mode;
- hooks;
- confirmation;
- audit.

Решение:

- trusted mode как explicit opt-in;
- typed host capability API;
- или внешний sandbox.

## Риск 3. Несовместимая restart policy

Prime Agent пытается сохранять продолжение работы. Intention Relay по charter не возобновляет external work автоматически.

Решение:

- сохранять completed child;
- прерывать unfinished child;
- explicit resume command позже.

## Риск 4. Tool loop explosion

Добавление tool calls превращает простой M4 stream executor в полноценный agent runtime.

Решение:

- отдельный `intention-agent-loop`;
- сначала один tool;
- затем typed registry;
- отдельные outcome tests.

## Риск 5. Harness ломает жёсткие правила

Модель может записать memory, которая противоречит policy.

Решение:

- harness только supplemental context;
- Rust hard invariants имеют приоритет;
- validate refinement before commit;
- base prompt immutable.

## Риск 6. Слишком ранняя интеграция

Текущая ветка M4 ещё не закрывает M5 tools и WorkspaceRoot. Полноценный RLM до этого будет либо:

- игрушечным;
- небезопасным;
- архитектурно обходящим собственные правила.

Решение:

```text
kernel proof-of-concept сейчас,
production RLM после tool/workspace boundaries.
```

## Риск 7. Python dependency and packaging drift

Если Python dependencies будут устанавливать динамически без pinned manifest, это конфликтует с quality policy Intention Relay.

Нужно заранее определить:

- supported Python version;
- lockfile/requirements policy;
- offline/online bootstrap;
- checksums or trusted package source;
- per-user kernel venv;
- rebuild policy after skill changes;
- Windows installation path;
- CI fixture strategy.

## Риск 8. Context leakage

IPython может прочитать больше, чем текущий model context или user-visible tool policy предполагает.

Нужно разделить:

```text
what Python can technically access
what parent model receives
what adapter sees
what durable audit stores
```

---

# 34. Что обязательно нужно протестировать

Интеграция должна следовать TTD/TTD policy Intention Relay. Одного compile pass недостаточно.

## Kernel contract tests

- successful kernel launch;
- hello/version negotiation;
- execution correlation;
- stdout/stderr/result separation;
- bounded output;
- malformed frame;
- oversized frame;
- cancellation;
- kernel crash;
- restart;
- session-to-kernel ownership;
- no raw path/secrets in safe errors.

## Agent-loop tests

- text-only terminal response;
- one tool call followed by second model request;
- multiple tool calls;
- invalid tool schema;
- tool rejection;
- tool timeout;
- cancellation during tool;
- provider error after tool;
- no retry after durable tool output;
- terminal event ordering.

## RLM tests

- child admission commits parent relationship;
- returned handle arrives only after commit;
- child gets own session/run;
- parent can continue while child runs;
- child completion is durable;
- child message delivery;
- child message queue;
- child follow-up;
- child cancellation;
- child deletion;
- duplicate spawn idempotency policy;
- sibling-name collision;
- recursion depth limit;
- child model selection policy;
- child usage attribution;
- parent snapshot contains bounded summary only;
- child replay does not leak another child/session.

## Recovery tests

- child admitted but not scheduled;
- child scheduled but provider not started;
- child provider stream interrupted by daemon restart;
- parent restart while child runs;
- kernel crash while child runs;
- child completes while parent disconnected;
- reconnect receives durable child state;
- no automatic external execution after restart;
- explicit resume creates new run if later supported.

## Harness tests

- local entry create/update/delete;
- global entry create/update/delete;
- scope collision;
- version increment;
- revision history;
- rollback;
- malformed proposal;
- forbidden kind/reference;
- immutable base prompt;
- hard policy cannot be overridden;
- atomic state plus event write;
- concurrent host and kernel edits;
- prompt projection size bound;
- stale entry handling.

## Security tests

- Python cannot bypass restricted host path policy;
- absolute path outside WorkspaceRoot is rejected;
- symlink escape is handled according to documented policy;
- Plan mode rejects project writes;
- credentials never reach Python request payload unnecessarily;
- provider credentials never appear in kernel output, events, snapshots, logs or adapter DTOs;
- malicious skill cannot claim privileged host request without authorization;
- MCP mutation is classified and audited;
- output is bounded;
- local sidecar endpoint has per-user permissions.

## Quality tests

New crates and features require updates to:

- `quality/architecture.toml`;
- coverage tiers;
- Cargo feature profiles;
- architecture checks;
- dependency policy;
- third-party notices;
- Makefile orchestration if new gates are necessary;
- documentation and roadmap.

For every implementation slice:

```text
contract tests first
architecture tests
implementation
make quick
focused tests
make verify
```

---

# 35. Final conclusion

Да, Prime Agent's RLM, IPython и Continual Harness можно встроить в `intention-relay`.

Но правильная формула не такая:

```text
Intention Relay + Prime Agent как готовая библиотека
```

а такая:

```text
Intention Relay Rust daemon
    +
Python IPython sidecar
    +
Rust-owned typed host bridge
    +
Rust-owned durable child/session state
    +
Rust-owned Continual Harness persistence
```

Роли компонентов должны быть разделены так:

```text
Rust:
  authoritative runtime
  sessions/runs
  SQLite
  provider execution
  tools
  WorkspaceRoot
  hooks
  policy
  cancellation
  child lifecycle
  protocol
  audit

IPython:
  persistent scratch state
  programmable orchestration
  data transformation
  Python skills
  RLM convenience API
  MCP convenience API
  harness client API

External model:
  reasoning
  planning
  tool selection
  delegation decisions
  refinement proposals
```

Самая важная архитектурная граница:

> IPython должен быть control plane, а не владельцем состояния и не обходом Rust policy layer.

Если эту границу выдержать, интеграция выглядит не только возможной, но и естественной.

Если нарушить её и позволить Prime Agent runtime параллельно владеть sessions, children, tools и persistence, получится две конкурирующие агентные системы, которые будут расходиться по lifecycle, recovery, state и безопасности.

Практический вывод:

```text
RLM: да, высокая совместимость.
IPython: да, лучше через Python sidecar.
Continual Harness: да, лучше реализовать authoritative storage в SQLite.
Прямое встраивание Prime Agent: нет.
Kernel POC: можно начать отдельно.
Production integration: после typed tools/WorkspaceRoot/hooks.
```

Полноценный RLM не является маленькой добавкой к текущему M4. Он требует как минимум:

- отдельного agent-loop слоя;
- kernel runtime;
- tool loop;
- child sessions;
- host-request protocol;
- durable harness storage;
- child messaging;
- usage attribution;
- explicit restart policy;
- security policy;
- новые DTO, domain events, storage tables и outcome tests.

Вероятнее всего это отдельный roadmap milestone после M5, либо крупная cross-milestone capability, которую нужно сначала формально внести в архитектуру, crate map, quality policy и M4/M5 decision register.
