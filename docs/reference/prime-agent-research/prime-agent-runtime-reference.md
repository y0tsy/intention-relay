# Prime Agent: полный разбор

## Краткий вывод

`prime-agent` — это не новая языковая модель и не обученная нейросеть. Это полноценная агентная платформа вокруг внешних LLM:

- CLI/TUI для работы с кодом и исследовательскими задачами;
- runtime выполнения действий;
- постоянный IPython/Python-контекст;
- рекурсивные дочерние агенты;
- долговременные сессии и восстановление после отключения;
- skills и MCP-интеграции;
- автоматическая компактизация контекста;
- цели, heartbeats, расписания и bounded autonomous mode;
- экспериментальный continual harness, который сохраняет полезные уроки и повторяемые процедуры.

В README это сформулировано как:

> Prime Agent: A Self-Improving RLM Agent

Но термин «self-improving» нужно понимать правильно: система не переобучает веса модели. Она улучшает внешний рабочий контур, сохраняя промпты, факты, навыки и спецификации субагентов.

---

# 1. Из чего состоит репозиторий

Репозиторий представляет собой TypeScript-монорепозиторий с Python runtime:

```text
packages/
  ai/              LLM-провайдеры и унифицированный streaming API
  agent/           базовый агентный цикл
  coding-agent/    основная логика Prime Agent
  tui/             терминальный UI

prime-agent-runtime/
  src/rlm/         Python-пакет для IPython kernel
```

Основной пакет — `packages/coding-agent`.

## `packages/ai`

Абстрагирует множество провайдеров и API:

- OpenAI Responses;
- OpenAI Completions;
- Anthropic Messages;
- Google Generative AI;
- Google Vertex;
- Amazon Bedrock;
- OpenAI Codex;
- Mistral;
- совместимые API и другие.

Все они приводятся к общему типизированному streaming-протоколу:

```text
text
thinking
tool_call
usage
done
error
aborted
```

Модель для Prime Agent — внешний ресурс. Сам Prime Agent не содержит «мозг» в виде весов. Он умеет подключаться к различным моделям и корректно работать с их особенностями: reasoning levels, cache, retries, abort, tool calls и provider-specific formats.

## `packages/agent`

Это общий агентный цикл.

Его схема примерно такая:

```text
user prompt
   ↓
LLM request
   ↓
assistant response
   ↓
tool calls?
   ├─ нет → завершение или continuation
   └─ да
       ↓
   execute tools
       ↓
   tool results
       ↓
   снова LLM
```

Цикл умеет:

- принимать новые user messages;
- стримить ответ модели;
- валидировать аргументы tool calls через TypeBox;
- выполнять несколько инструментов параллельно или последовательно;
- прерываться через `AbortSignal`;
- принимать steering-сообщения во время работы;
- принимать follow-up-сообщения после завершения текущей работы;
- запускать continuation policy;
- обрабатывать ошибки и частично полученные ответы.

## `packages/tui`

Терминальный интерфейс предоставляет:

- редактор;
- очереди сообщений;
- просмотр дерева сессии;
- выбор модели;
- управление goals, heartbeats и autonomous mode;
- отображение дочерних агентов;
- подключение расширений;
- markdown и изображения.

## `prime-agent-runtime`

Это небольшой Python-мост, а не второй агентный движок.

Он предоставляет IPython-коду:

```python
rlm(...)
rlm.find_models(...)
rlm.list_subagents()
rlm.delete_subagent(...)
rlm.harness
host_request(...)
```

Сам цикл агента остаётся в TypeScript. Python только передаёт typed requests обратно в host через Jupyter comm.

---

# 2. Главная идея: Recursive Language Model

Главная архитектурная идея — RLM, Recursive Language Model.

В обычном агенте модель получает набор инструментов:

```text
read_file
write_file
bash
search
browser
...
```

В Prime Agent модель в основном получает один инструмент:

```text
ipython
```

А уже внутри IPython она программно вызывает всё остальное:

```python
from pathlib import Path

files = list(Path(".").rglob("*.ts"))
large_files = [path for path in files if path.stat().st_size > 10000]
```

Или:

```python
%%bash
npm run check
```

Или:

```python
old = Path("src/app.ts").read_text()
new = old.replace("old", "new")
Path("src/app.ts").write_text(new)
```

Или:

```python
handle = await rlm(
    "Review the authentication implementation and report findings to the parent",
    name="auth-reviewer",
)
```

То есть модель не просто вызывает инструменты по одному. Она может написать короткую программу, которая:

- собирает данные;
- сохраняет их в переменные;
- фильтрует;
- агрегирует;
- запускает несколько операций;
- условно повторяет действия;
- делегирует части задачи;
- анализирует результаты;
- сохраняет состояние между вызовами.

Это существенно мощнее простого списка stateless tools.

---

# 3. Почему постоянный IPython-контекст важен

Обычный tool-call часто выглядит так:

```text
модель → read(file) → результат
модель → search(pattern) → результат
модель → read(file) → результат
```

В Prime Agent модель может построить собственный рабочий pipeline:

```python
files = ...
matches = ...
summaries = ...
failed = ...
```

Переменные, импорты, функции, async-задачи и промежуточные результаты переживают следующие вызовы и компактизацию контекста.

## 3.1. Меньше повторного чтения

Модель не обязана каждый раз заново просить host прочитать то же самое. Она может сохранить:

```python
config = Path("config.toml").read_text()
```

и использовать `config` позже.

## 3.2. Произвольная композиция

Вместо фиксированной комбинации инструментов модель может написать код:

```python
reports = {}

for path in Path("packages").rglob("*.ts"):
    text = path.read_text()
    if "createAgentSession" in text:
        reports[str(path)] = text.count("createAgentSession")
```

Это уже не просто «вызов инструментов». Это вычислительная среда управления исследованием.

## 3.3. Возможность сохранять handle'ы

Модель может сохранить handle дочернего агента:

```python
review = await rlm("Review the API", name="api-reviewer")
```

Позже:

```python
children = await rlm.list_subagents()
```

И отправить продолжение конкретному ребёнку:

```python
await agent_message.send(
    "Also check authorization boundaries",
    receiver_role="child",
    receiver_name=review.name,
)
```

---

# 4. Как устроен настоящий runtime

Архитектура разделена на несколько процессов.

```text
TUI / CLI / RPC client
        ↓
AgentConnection
        ↓
Daemon supervisor
        ↓
Session worker
        ↓
AgentSession
        ├─ Agent loop
        ├─ provider calls
        ├─ scheduler
        ├─ IPython kernel
        └─ RLM child runtimes
```

## 4.1. Клиент

Клиент отвечает за:

- UI;
- клавиатуру;
- вывод;
- подключение и переподключение;
- локальные настройки отображения.

Он не владеет выполнением агентной задачи.

## 4.2. Supervisor

Daemon supervisor отвечает за:

- маршрутизацию;
- обнаружение агентов;
- attach/detach;
- worker lifecycle;
- доставку межагентных сообщений;
- восстановление после падения worker;
- протокол и generation-aware replay;
- command journal;
- backpressure.

## 4.3. Worker

Один worker владеет одним деревом:

```text
root agent
  ├─ child agent
  ├─ child agent
  └─ child agent
```

В worker находятся:

- root `AgentSession`;
- scheduler;
- kernels;
- дочерние runtime;
- session persistence.

Закрытие терминала не останавливает worker. Клиент отключается, а задача продолжает выполняться.

## 4.4. Kernel

IPython kernel запускается отдельным Python-процессом.

TypeScript соединяется с ним по Jupyter protocol через ZeroMQ:

- shell;
- iopub;
- control;
- heartbeat.

Сообщения подписываются HMAC-SHA256. Kernel работает только на loopback.

Но это не sandbox.

Документация прямо предупреждает: model-generated Python и project commands run with worker OS permissions.

То есть модель может выполнять команды с правами пользователя.

Разделение процессов предназначено для:

- lifecycle isolation;
- recovery;
- возможности перезапустить kernel;
- сохранения состояния;
- корректной обработки зависших операций.

Это не защита от вредоносного кода.

---

# 5. Как работает рекурсивный субагент

Вызов:

```python
handle = await rlm("Inspect the API", name="api-reviewer")
```

проходит примерно так:

```text
Python rlm()
   ↓
Jupyter comm: host.request
   ↓
KernelManager
   ↓
AgentSession host handler
   ↓
validate prompt/name/model
   ↓
create child runtime
   ↓
return spawn handle
```

Возвращаемый объект содержит:

```text
rlm_child_id
name
session_dir
model
```

Ключевой момент:

```text
rlm() возвращает admission handle, а не ответ ребёнка
```

Он не блокируется до завершения дочерней задачи.

Child — это полноценный `AgentSession`, а не отдельный упрощённый вызов модели. Он наследует:

- модель;
- provider configuration;
- retry policy;
- thinking configuration;
- skills;
- tools;
- resource loader;
- transport;
- persistence policy.

Но у ребёнка собственные:

- контекст;
- session directory;
- transcript;
- runtime;
- lifecycle;
- usage accounting.

## 5.1. Как ребёнок возвращает результат

Через сообщение:

```python
await agent_message.send(
    "Authentication review complete: ...",
    receiver_role="parent",
)
```

Или через файл:

```python
Path("review-result.md").write_text(report)
```

Родитель затем получает обычное agent message либо читает файл.

Это важное решение: родитель не обязан передавать весь контекст ребёнка обратно в себя. Ответ может быть маленьким, а тяжёлое исследование остаётся внутри дочерней сессии.

## 5.2. Глубина рекурсии

По умолчанию:

```text
root → children
```

Дети не создают внуков, если `RLM_MAX_DEPTH` не увеличен.

Host проверяет depth до создания ребёнка, а Python shim также валидирует часть условий.

## 5.3. Управление детьми

Есть полноценный registry:

```python
children = await rlm.list_subagents()
```

Поддерживаются:

- `running`;
- `completed`;
- `error`;
- восстановление registry после restart/compaction;
- retained completed children;
- follow-up;
- cancellation;
- parent-owned lifecycle.

## 5.4. Usage attribution

Расход ребёнка не теряется.

Host асинхронно атрибутирует usage к parent assistant turn:

```text
parent turn
  ├─ own usage
  └─ child usage
```

При этом:

- общий billable usage учитывает ребёнка;
- context-window usage родителя не раздувается ошибочно;
- context-tree может показывать собственные расходы узлов отдельно;
- после reload attribution восстанавливается из transcript.

Это признак зрелой реализации, а не просто `spawn("agent")`.

---

# 6. Что делает систему «умной»

Самая точная формулировка:

> Prime Agent не добавляет разум в модель. Он превращает модель в длительно работающую, программируемую, рекурсивную и самопроверяющуюся систему.

Основные источники «умности» следующие.

## 6.1. Агентный цикл

Модель может:

```text
подумать
→ вызвать IPython
→ увидеть результат
→ скорректировать план
→ вызвать следующий шаг
→ проверить результат
→ исправить ошибку
→ продолжить
```

Это iterative execution loop, а не один prompt-response.

## 6.2. Программируемая работа с контекстом

Вместо того чтобы передавать модели всё сразу, она может держать большой объём данных в Python:

```python
all_logs = ...
relevant = [line for line in all_logs if ...]
```

В контекст модели попадают только нужные excerpts и summaries.

Это особенно полезно для:

- больших репозиториев;
- длинных логов;
- datasets;
- research tasks;
- большого числа файлов;
- длительных задач.

## 6.3. Рекурсивная декомпозиция

Родитель может распределить работу:

```text
child 1: архитектура
child 2: тесты
child 3: security
child 4: документация
```

Каждый получает отдельный контекст. Родитель делает fan-out/fan-in.

Это снижает давление на один context window и позволяет выполнять независимые исследования параллельно.

## 6.4. Self-verification

В автономном режиме можно задать quality gates:

```bash
prime-agent \
  --autonomous \
  --autonomous-gate "npm run check" \
  --autonomous-gate "npm test"
```

После ответа модели host запускает проверки.

Если gate падает:

1. вывод ошибки ограниченно передаётся модели;
2. host inject'ит continuation;
3. модель исправляет работу;
4. gate запускается снова;
5. если workspace не изменился, одинаковый gate не гоняется бессмысленно;
6. есть retry/turn/token/time limits.

То есть система не считает красивый финальный текст доказательством успеха.

## 6.5. Persistent goals

Goal — это durable objective:

```text
Ship the release and verify every artifact
```

Состояние включает:

- objective;
- status;
- token budget;
- tokens used;
- elapsed time;
- continuation count;
- error state.

Модель должна явно вызвать:

```python
await goal.complete()
```

Само утверждение «готово» не считается завершением цели.

## 6.6. Context compaction

Когда окно контекста приближается к пределу, Prime Agent:

1. выбирает старую часть;
2. сериализует сообщения и tool calls;
3. генерирует структурированное summary;
4. сохраняет summary как session entry;
5. оставляет последние сообщения;
6. продолжает работу.

Summary имеет структуру:

```markdown
## Goal
## Constraints & Preferences
## Progress
## Key Decisions
## Next Steps
## Critical Context
<read-files>
...
</read-files>
<modified-files>
...
</modified-files>
```

При этом Python kernel state продолжает жить.

То есть:

```text
conversation memory может быть сжата,
Python working state сохраняется отдельно.
```

Это одна из сильных сторон архитектуры.

## 6.7. Continual harness

Harness хранит четыре типа записей:

```text
prompt
memory
skill
subagent
```

Примерно это означает:

- `prompt`: дополнительное поведенческое правило;
- `memory`: факт, решение, предпочтение или прошлый failure;
- `skill`: повторяемая процедура с Python API;
- `subagent`: переиспользуемая роль делегата.

`/refine` вызывает отдельный model-backed анализ текущей trajectory. Он должен предложить небольшие JSON-изменения:

```json
{
  "summary": "...",
  "rationale": "...",
  "expectedOutcome": "...",
  "edits": []
}
```

Изменения должны быть:

- evidence-backed;
- минимальными;
- локальными по умолчанию;
- откатываемыми;
- не меняющими immutable base system prompt.

Это не gradient learning. Это online editing внешней памяти и orchestration policy.

## 6.8. Skills с progressive disclosure

При старте в system prompt попадают только:

- имя;
- тип;
- описание;
- путь.

Полный `SKILL.md` модель читает только если задача подходит.

Это экономит context window.

Skills бывают двух типов.

### Markdown skills

Они содержат инструкции.

### Python-backed skills

Они содержат:

```text
SKILL.md
pyproject.toml
src/<import_name>/__init__.py
```

И автоматически устанавливаются в managed kernel environment.

Например, встроенные skills:

- `websearch`;
- `goal`;
- `compact`;
- `refine`;
- `agent-message`;
- `agent-observe`;
- `attach-image`;
- `prime-intellect`;
- `notion`;
- `linear`.

У Python skill может быть async API:

```python
await websearch("query")
```

Или:

```python
await linear.list_issues(team="Engineering")
```

---

# 7. MCP-интеграции

MCP не добавляется как новый набор отдельных model tools.

Вместо этого MCP-сервер превращается в Python-backed skill:

```python
import linear

tools = await linear.list_tools()
issues = await linear.list_issues(team="Engineering")
```

Преимущества подхода:

- модель всё равно работает через один IPython-инструмент;
- API MCP можно исследовать программно;
- Python может комбинировать несколько вызовов;
- результаты можно фильтровать и агрегировать;
- schema приходит от сервера;
- credentials остаются под контролем TypeScript host.

Система предусматривает:

- Linear;
- Notion;
- пользовательские HTTP MCP servers.

MCP credentials хранятся в `auth.json`, OAuth выполняется host-частью, а вызов server tools происходит из kernel.

Текущее ограничение: документация указывает, что `stdio` MCP servers не подключены к kernel path, поддерживаются remote HTTP endpoints.

---

# 8. Heartbeats и расписания

Prime Agent может продолжать работу без открытого terminal UI.

Есть несколько механизмов.

## User heartbeat

```text
/heartbeat every 10m Check the deployment
```

## RLM heartbeat

Модель может создать несколько внутренних периодических задач:

```python
await rlm_heartbeat.create(
    "Check whether tests finished",
    interval="5m",
    label="tests",
)
```

## Общие schedules

```bash
prime-agent schedule add worker "in 30m" -- "Check the benchmark"
prime-agent schedule add worker "0 9 * * 1-5" -- "Review open work"
```

Состояние jobs сохраняется на сессию. Due ticks сначала claim'ятся, чтобы падение процесса не приводило к неконтролируемому повтору одной и той же задачи.

---

# 9. Межагентная коммуникация

Агенты могут общаться напрямую через daemon:

```python
await agent_message.send(
    "Please recheck the migration",
    receiver_role="sibling",
    receiver_name="migration-reviewer",
)
```

Разрешённая область — nuclear family:

```text
parent
siblings
direct children
```

Свободное глобальное общение со всеми агентами запрещено.

Есть ограничения:

- размер сообщения;
- rate limit;
- pending queue;
- sender identity выставляется daemon, а не Python-кодом;
- доступны delivery modes;
- сообщение может быть `delivered` или `queued`.

Это не просто messaging convenience. Это механизм распределённой координации внутри дерева задач.

---

# 10. Автономность ограничена намеренно

Autonomous mode не означает бесконечный runaway loop.

По умолчанию есть лимиты:

```text
maxContinuations: 3
maxTurns: 12
maxTokens: 80_000
timeoutMs: 30 minutes
```

Дополнительно есть:

- quality gates;
- retry count;
- gate timeout;
- subprocess cancellation;
- bounded output;
- проверка изменения workspace.

Важное правило:

> достижение лимита не означает успех задачи.

Если gate не задан, autonomous mode может продолжать до лимита, но это не доказывает, что работа корректна.

Если gate задан, host проверяет фактический внешний результат.

---

# 11. Сессии и долговременность

Сессии сохраняются как JSONL с деревом entries:

```text
id
parentId
```

Это позволяет:

- ветвиться;
- возвращаться к старому состоянию;
- fork'ать сессию;
- clone'ить текущую ветку;
- делать tree navigation;
- сохранять compaction entries;
- сохранять child usage attribution;
- сохранять goal state;
- сохранять custom messages;
- сохранять kernel snapshots.

Артефакты могут выглядеть так:

```text
~/.prime/agent/
  sessions/
    <session>.jsonl
  session-artifacts/
    <session>/
      kernel-state.dill
      kernel-state.json
      scheduled-jobs.json
      harness/
        harness_state.json
      sub-xxxxxxxx/
        <child-session>.jsonl
```

После перезапуска можно восстановить:

- transcript;
- kernel namespace;
- schedules;
- harness;
- retained children;
- worker runtime.

Это делает Prime Agent ближе к resident process, чем к одноразовому CLI-скрипту.

---

# 12. Насколько хорошо всё это покрыто тестами

В репозитории около:

```text
414 test files
```

Особенно много тестов у `coding-agent`.

Проверяются:

- базовый agent loop;
- tool execution;
- abort;
- retries;
- compaction;
- session tree;
- kernel startup;
- kernel abort;
- kernel snapshots;
- kernel state roundtrip;
- RLM recursion;
- child lifecycle;
- child model selection;
- agent messaging;
- agent observation;
- goals;
- autonomous mode;
- heartbeats;
- daemon protocol;
- worker recovery;
- session leases;
- MCP manager;
- RPC;
- ACP;
- extensions;
- queues;
- action races;
- serialized refine;
- daemon restart;
- snapshot transfer;
- subagent retention.

Есть специализированные файлы вроде:

```text
agent-session-recursion.test.ts
kernel-state-roundtrip.test.ts
kernel-agent-message-skill.test.ts
kernel-agent-observe-skill.test.ts
kernel-goal-skill.test.ts
kernel-rlm-heartbeat-skill.test.ts
agent-session-autonomous.test.ts
agent-session-goal.test.ts
agent-session-refine-skill.test.ts
daemon-supervisor-lazy-subagents.test.ts
worker-recovery-journal.test.ts
```

Это говорит о том, что RLM и long-running functionality — не только маркетинговые документы. Они встроены в тестируемую модель runtime.

Однако в клонированном репозитории зависимости ещё не установлены:

```text
node_modules absent
```

Поэтому тесты и build в рамках исследования не запускались. Для этого сначала потребовался бы `npm ci`, который скачивает зависимости и изменяет рабочую директорию. Исследование ограничивалось чтением исходников, тестов, документации и Git history.

---

# 13. Что это не делает

## 13.1. Не обучает модель на лету

В репозитории нет механизма:

```text
trajectory → gradients → update neural weights
```

Нет локального fine-tuning pipeline, optimizer, checkpoint training loop или weight update.

`RLM-1` упоминается в TODO как будущая или экспериментальная возможность, например:

```text
reconsider whether persistent kernel is needed once RLM-1 weights land
```

Но в текущем коде runtime — это orchestration layer, а не обучающая система.

## 13.2. Не гарантирует истину

LLM всё ещё может:

- ошибиться;
- неправильно интерпретировать задачу;
- испортить файл;
- сделать неверный вывод;
- неправильно выбрать delegation strategy.

Система лишь даёт ей больше возможностей:

- проверить;
- повторить;
- сравнить;
- вызвать тесты;
- привлечь других агентов;
- сохранить ошибку;
- продолжить позже.

## 13.3. Не является security sandbox

Python kernel и shell выполняются с правами пользователя.

Особенно опасны:

- сторонние skills;
- extensions;
- MCP endpoints;
- инструкции из непроверенных репозиториев;
- model-generated shell commands.

Документация прямо советует использовать внешний sandbox для недоверенного кода.

## 13.4. Не является полной distributed system

Daemon architecture очень продвинута, но распределение ограничено локальным процессным деревом. Это не кластер с глобальным scheduler'ом и не multi-machine actor system.

---

# 14. Сильные стороны проекта

## 14.1. Сильная архитектурная декомпозиция

Разделение:

```text
provider
agent loop
session
runtime
kernel
daemon
TUI
persistence
```

позволяет заменять части независимо.

## 14.2. Один мощный интерфейс вместо множества tools

IPython становится универсальным control plane.

Это уменьшает:

- tool schema overhead;
- количество отдельных protocol paths;
- дублирование логики;
- количество специальных model tools.

## 14.3. Настоящая долговременность

Работа не умирает при закрытии терминала. Есть:

- daemon;
- worker;
- session artifact;
- kernel snapshot;
- scheduler;
- reconnect;
- recovery journal.

## 14.4. Рекурсивность является частью host runtime

Subagent — не «ещё один API call». Это полноценный агент со своими:

- transcript;
- provider call;
- tools;
- session;
- lifecycle;
- accounting.

## 14.5. Самопроверка через внешние evidence

Quality gates и test commands — сильнее, чем обычное «модель сказала, что всё готово».

## 14.6. Ограниченная, но реальная внешняя память

Continual harness позволяет сохранять:

- repeated failure;
- durable fact;
- reusable procedure;
- reusable delegation role;
- narrow behavior rule.

Причём предусмотрены:

- local/global scopes;
- versioning;
- refinement history;
- rollback;
- filesystem synchronization;
- защита immutable base prompt.

---

# 15. Слабые стороны и риски

## 15.1. Сложность

Система очень большая для CLI-агента:

```text
1085 файлов только в packages/runtime tree
```

Чем больше subsystem'ов, тем больше:

- race conditions;
- recovery edge cases;
- protocol compatibility burden;
- состояний, которые трудно объяснить пользователю.

## 15.2. Большая зависимость от качества модели

RLM не заменяет reasoning model. Модель должна сама догадаться:

- когда писать Python;
- когда делать цикл;
- когда делегировать;
- как собрать результаты;
- когда запускать проверки;
- когда сохранять memory;
- когда не создавать лишнего child.

Runtime предоставляет affordances, но не гарантирует, что модель воспользуется ими оптимально.

## 15.3. Memory pollution

Continual harness улучшает поведение, но потенциально может сохранять:

- неверные выводы;
- слишком узкие правила;
- устаревшие предположения;
- плохие delegation patterns.

Rollback и evidence requirements снижают риск, но не устраняют его полностью.

## 15.4. Несколько видов памяти

Одновременно существуют:

- conversation transcript;
- session tree;
- compaction summary;
- Python kernel namespace;
- kernel snapshot;
- local harness;
- global harness;
- child registry;
- scheduled jobs;
- goal state.

Это мощно, но когнитивно сложно. Ошибка в том, где искать состояние, может привести к неправильной работе агента.

## 15.5. Безопасность

Полный доступ к OS — фундаментальный риск. Extension и skill model:

```text
package installation
→ arbitrary code
→ model can invoke it
```

MCP также расширяет внешний trust surface.

---

# Итоговая оценка

Prime Agent — это зрелый programmable agent runtime, а не просто чат-обёртка над GPT или Claude.

Его «умность» складывается из пяти слоёв:

```text
1. Сильная внешняя LLM
2. Iterative agent loop
3. Persistent Python control environment
4. Recursive multi-agent orchestration
5. Persistent memory + verification + recovery
```

Особенно важна комбинация:

```text
LLM
  → Python program
    → files / shell / skills / MCP
      → child agents
        → messages / artifacts
          → verification gates
            → continuation
```

Самая точная характеристика:

> Это операционная система для длительной работы языковой модели, где IPython выступает программируемым control plane, RLM — механизмом рекурсивной декомпозиции, daemon — слоем жизненного цикла, а continual harness — редактируемой внешней памятью и policy layer.

Он не становится умным за счёт самостоятельного обучения весов. Он становится существенно более полезным за счёт того, что модель получает:

- память между действиями;
- возможность писать управляющие программы;
- возможность делегировать;
- возможность продолжать работу после disconnect;
- возможность проверять себя;
- возможность сохранять повторяемые уроки;
- возможность работать с контекстом намного эффективнее обычного tool-calling агента.
