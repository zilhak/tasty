<!-- source-hash: 2bdaf53af855 -->
<a id="task-dag"></a>

# Task workflows (DAG)

Run tests after a build finishes, then pass the results to the next task. Register each step as a **task** and connect it to the work it depends on. Tasty handles the order and failure policy. These connected tasks form a **DAG**, a graph of task dependencies.

Create and manage tasks with the `tasty` CLI. To follow them in the app, see [Watching progress](#watching-progress).

Pass the task's workspace as `--workspace-id` in each command. The command uses that workspace even while you are viewing another one.

## Starting the runner

You can create tasks, but nothing runs while the runner is off. Start it once per Workspace.

```sh
tasty agent task-run --workspace-id 2 --action start
tasty agent task-run --workspace-id 2 --action status
tasty agent task-run --workspace-id 2 --action stop
```

Restarting Tasty does not start the runner again automatically. The tasks themselves are still there, so `start` again and it picks up where it left off. When the runner is stopped, the runner pill on the DAG screen says so, and hovering it shows the command that starts it again. In a DAG tab that command is also spelled out on the header line.

## Creating a task

```sh
BUILD=$(tasty agent task-create --workspace-id 2 --name build \
  --command '{"kind":"run","command":["cargo","build"]}' | jq -r .id)
```

Creating one returns a task ID. Use that ID to define dependencies and to query state.

| Command kind | What it does |
|---|---|
| `run` | Runs the specified command. It is a background process that does not occupy a terminal, and it carries up to the last 64KiB each of standard output and standard error in the result. Interactive programs do not fit here. It gets the environment Tasty was started from, with the exceptions below the table |
| `custom` | Turns one of Tasty's own actions into a task. Things that create a terminal, such as spawning a child agent, belong here |
| `reduce` | Merges the results of several tasks into one |
| `wait_barrier` | Waits until all the signals have gathered at a barrier |

You can also pull the command JSON out into a file and pass it as `--command @build.json`.

A `run` command, the postprocess command below and a reducer that merges results with a shell command get the environment Tasty was started from. If you started Tasty inside Claude Code, the variables that point at that session are not passed on (the same as in the terminal). If you started Tasty from another Tasty's terminal, `TASTY_SESSION_TOKEN`, `TASTY_SURFACE_ID`, `TASTY_PARENT_HOME` and `TASTY_AGENT_ID`, which point at that Tasty, are not passed on either, so a `tasty` call inside the command reaches this Tasty.

## Order and failure handling

```sh
TEST=$(tasty agent task-create --workspace-id 2 --name test \
  --command '{"kind":"run","command":["cargo","test"]}' \
  --depends-on "$BUILD" --on-failure abort | jq -r .id)
```

Every task listed in `--depends-on` has to finish before this task becomes ready. A graph that would form a cycle is rejected at creation time.

| Failure policy | Meaning | Where you attach it |
|---|---|---|
| `abort` (default) | If a task it depends on fails, this task is skipped. Everything below it is skipped too | The depending side |
| `continue_downstream` | This task runs even if a task it depends on failed | The depending side |
| `fallback:<task ID>` | If this task fails, that task is woken up instead | The side that can fail |

It is easy to get the placement wrong. `abort` and `continue_downstream` have to be attached to the **following** task for them to count toward that task's readiness decision, while `fallback` has to go the other way, on the task that **can fail** itself. Using a policy on the wrong task will not give you the intended failure handling.

A task used as a fallback has to be created before the main task. To stop the runner from running it first in the meantime, create the fallback with `--reserved-for-fallback`. The reservation prevents it from running before the main task is created. Once linked, it continues waiting until the main task fails and activates the fallback.

## Passing results from earlier tasks

`--depends-on` only ties order together. To pass an earlier task's result as a later task's **input**, use a placeholder.

```
${task.<task ID>.output}          the whole result
${task.<task ID>.output/stdout/text}   one value inside the result
```

The real value goes in that spot when the task is dispatched. Use it for values you cannot know at creation time and that are only settled by running — a flow that spawns a child agent and then talks to that child, for instance. The task you reference must be listed in `--depends-on`, otherwise creation is rejected. The shape of the value is preserved. A string that is nothing but a single placeholder turns into a number if the value is a number.

## Sending a typed group of tasks at once

To fix the types of results too, bundle tasks into one graph and send it. The whole graph is checked first, so if anything is wrong no task is created and the error says which spot in which task is wrong. No task runs before every task exists. One graph holds up to 1000 tasks; split larger sets into several graphs.

```sh
tasty agent task-graph-submit --workspace-id 2 --graph @graph.json --dry-run   # check only
tasty agent task-graph-submit --workspace-id 2 --graph @graph.json
```

```json
{"contract_version": 2,
 "tasks": [
   {"id": "build", "command": {"kind": "run", "command": ["cargo", "build"]}},
   {"id": "report", "command": {"kind": "run", "command": ["notify"]},
    "input_schema": {"type": "object", "fields": {"code": {"type": "int64"}}},
    "bindings": {"code": {"from_task": "build"}},
    "input_mapping": {"args": ["/code"]}}]}
```

You choose each task's `id`. `bindings` say where inputs come from: a fixed value (`literal`), one value from an earlier task's result (`from_task` with `pointer`), or whichever of a task and its fallback ran (`one_of`). Types that do not fit are rejected when you send the graph. Values go only where `input_mapping` says. `args` appends them to the command one argument each, and `stdin: true` writes the whole input as JSON to standard input. `$(...)` or spaces inside a value are never interpreted again. The values actually passed are in the task's `input_snapshot`. Tasks in such a graph do not use the placeholders above.

To run a task's result through another command (a judge or summary tool, say) and use that as the result, add `postprocess`. When the main work succeeds, the command runs and its standard output becomes the task's result.

```json
{"id": "judge",
 "command": {"kind": "run", "command": ["make-draft"]},
 "output_schema": {"type": "enum", "values": ["pass", "revise"]},
 "postprocess": {"command": ["judge-cli", "--json"],
                 "stdin": {"draft": {"from": "raw", "pointer": "/execution/stdout/text"}},
                 "stdout": {"pointer": "/verdict"},
                 "timeout_ms": 120000,
                 "retry": {"max_retries": 1}}}
```

- The command runs as is, without a shell. If you need one, write it out, as in `["sh", "-c", "..."]`. Sign-in uses the environment Tasty was started from. If you started Tasty inside Claude Code, the variables that point at that session are not passed on (the same as in the terminal). If you started Tasty from another Tasty's terminal, variables such as `TASTY_SURFACE_ID` that point at that Tasty's terminal are not passed on either.
- `stdin` sets the fields of the JSON written to standard input. `from` is `input` (the task's input), `raw` (the main work's result) or `artifacts`, and `pointer` picks a value inside it.
- By default standard output must be exactly one JSON value. With `"stdout": {"format": "text"}` the text is taken as is. Progress logs sent to standard error stay out of the result.
- `timeout_ms` is required. When it runs out or the task is cancelled, the command and the processes it started are stopped.
- With `retry`, a failure reruns only the postprocess that many times; the main work does not run again. Once the retries are used up, the task fails and the tasks after it follow their failure policies as for any failure. There is no way to rerun only the postprocess. To try again, rerun the task from the main work with `task-retry`.
- While a task with declared types runs, its step appears at the end of its `task-list` line and on the `phase:` line of `task-get`. During the main work it is `executing`.
- Until the postprocess finishes, the task shows as running and the next tasks wait. `task-get` shows `phase: postprocessing (run 1)` on the line after `state: running` (`phase: retry_wait (run 2)` while it waits to retry).
- If the postprocess fails, the `state` line in `task-get` gives the reason. The lines under it give the last run's cause and exit code, such as `postprocess: run 2 failed (nonzero_exit), exit_code 3`, and the causes of the runs that were retried (`postprocess retried after: ...`).
- If Tasty quits or restarts while a postprocess runs, that postprocess is not run again and the task fails. To try again, rerun the task with `task-retry`.
- If Tasty is killed (with `kill`, for example), other processes the postprocess command started can be left running. On Linux the postprocess command itself gets a termination signal, on Windows they end with Tasty, and on macOS they are all left running.

While Tasty runs on [temporary memory](cli.md#memory-shared-between-agents) because it could not open its memory file, a graph would not survive a restart, so sending it as is gets refused. For a graph that may be lost on restart, add `"durability": "best_effort"`.

### Choosing the next task from a result

Give a task `transitions` and it picks which tasks run next from its result. In the example below, `review` runs only `ship` when its verdict is `pass`, only `fix` when it is `revise`, and only `human` otherwise.

```json
{"id": "review", "command": {"kind": "run", "command": ["review-tool"]},
 "output_schema": {"type": "object", "fields": {
   "verdict": {"type": "enum", "values": ["pass", "revise", "review"]}}},
 "transitions": {
   "cases": [
     {"when": {"compare": {"path": "/verdict", "op": "eq", "value": "pass"}}, "to": ["ship"]},
     {"when": {"in": {"path": "/verdict", "values": ["revise"]}}, "to": ["fix"]}],
   "otherwise": ["human"]}}
```

- A condition compares one spot in the result (`path`) with a value (`compare`: `eq`, `ne`, `lt`, `le`, `gt`, `ge`) or checks that it is one of a list (`in`). Combine them with `all`, `any` and `not`. Conditions that do not fit the result's type are rejected when you send the graph. Decimals are compared only as ranges.
- By default exactly one condition may hold. If two hold, the task ends as failed. To run every match, add `"mode": "all_matches"`.
- Always say what happens when nothing holds: tasks to run (`otherwise`) or run nothing (`"no_match": "finish"`).
- Tasks that were not chosen, and tasks reached only through them, end as "not selected", not as failures. A task where branches meet waits only for the branches that ran. If a branch that ran fails, that failure still passes on.
- Taking a required input from a branch that may not run is rejected when you send the graph. If each branch produces the value in a different task, take it with `one_of`. If the value may be missing, make the input field optional or give it a default.
- A task's `route` shows the tasks it chose, and a task that was not chosen shows why in `skip`. When every task succeeded or was not selected, the DAG shows as succeeded. A failed task whose fallback succeeded in its place does not make the DAG fail.
- When one branch fails, the DAG still shows an in-progress state (running, ready or waiting) while another branch can go on. A task whose result became unknown (`unknown`) only moves on after you retry or cancel it, so it does not count as in progress. When everything has finished, a DAG where some branch succeeded all the way to its last task is `partially_failed`, and one with no such branch is `failed`. A single line of tasks where only the early tasks succeeded and the last one failed or was skipped is `failed`. A last task that failed but whose fallback succeeded in its place counts as a success. This appears as `rollup_state` in `agent dag-list`. The Task DAGs list does not have its own mark for this state yet and shows it with the `?` mark and the label **Partially failed**, and while any status filter is on, partially failed DAGs do not appear in the list.

### Asking an agent and taking its answer as the result

An `agent` task sends one instruction to a Claude or Codex session and takes the answer as the task's result. The result can feed the next task's input or the branch conditions above.

```json
{"id": "review",
 "command": {"kind": "agent", "provider": "claude", "workspace_id": 2,
             "session": {"kind": "existing", "surface_id": 12},
             "instruction": "Review the changes on this branch",
             "timeout_ms": 1800000},
 "output_schema": {"type": "enum", "values": ["approve", "revise"]}}
```

- The `workspace_id` in `command` is the workspace a new session opens in. `task-submit` hands the value to the workspace the task lives in, and the guidance appended to the instruction carries that number.
- With `session` set to `{"kind": "new", "parent_surface": <surface>}`, a new session starts under that surface and gets the instruction. With `{"kind": "existing", "surface_id": <surface>}`, the instruction goes to a session that is already open.
- An open session gets the instruction only when it is idle. If you are talking to it, the task waits until you finish, so the conversations do not mix. A one-line marker for that run is added to the end of the instruction, and only a turn started with that marker counts as the task's turn. Tasks that use the same session run one after another.
- Without a result type, the last answer of the turn becomes a string result.
- If the Claude session has a Stop gate, the turn does not end while the gate keeps the response going. The task finishes with the last answer after the gate lets the response end. If you need the first answer to the instruction, set a result type and have the agent hand it in with `task-submit`.
- With a result type, as in the example, the agent has to hand in a value with `tasty agent task-submit`. How to do that, the type and a token used only for that run are added to the end of the instruction. A value with another token is refused, so a value handed in by mistake for another task or an earlier run does not get mixed in. The value is checked right away and becomes the result when the turn ends. A verdict such as `revise` is a successful result.
- A task that takes input adds `"input_mapping": {"input_block": true}`. The input is appended to the instruction as JSON.
- The task fails when the turn ends without an answer, the turn ends with an error, the session ends, or `timeout_ms` passes. `task-get` shows which one happened. When the turn ends without the value being handed in, the first 2000 characters of the turn's last answer stay in the failure record.
- While the agent waits for input such as a permission prompt, the task stays running and `task-get` shows a `phase: awaiting_input` line and a line like `agent session: claude surface 12, awaiting input since …`.
- Finishing or cancelling the task does not close the session. If Tasty restarts, running agent tasks fail.
- If a new session does not come up within 30 seconds, the task fails. A session that comes up later is closed by the task. A session that was already open is never closed.
- When Codex runs in a sandbox that blocks the network, `task-submit` cannot reach Tasty, so a task with a result type fails. Tasks without a result type (the last answer is the result) are not affected.

### Example: implement, review, branch on the verdict, and meet again

This graph puts the features above together. `implement` hands out a sentence describing its change, and `review` takes that sentence as input and returns a verdict with a confidence. A `pass` with confidence 0.9 or more goes to `ship`, a `revise` with confidence 0.9 or more goes to `fix`, and anything else goes to `human_review`. The three branches meet again at `report`. Replace `make-change`, `review-tool` and the rest with your own tools. To hand the work to an agent, use the `agent` task above instead.

```json
{"contract_version": 2,
 "types": {"ReviewResult": {"type": "object", "fields": {
   "verdict": {"type": "enum", "values": ["pass", "revise", "review"]},
   "confidence": {"type": "float64", "min": 0, "max": 1}}}},
 "tasks": [
  {"id": "implement", "command": {"kind": "run", "command": ["make-change"]},
   "output_schema": {"type": "string"},
   "postprocess": {"command": ["summarize-tool"], "timeout_ms": 10000,
                   "stdin": {"text": {"from": "raw", "pointer": "/execution/stdout/text"}}}},
  {"id": "review", "command": {"kind": "run", "command": ["review-tool"]},
   "input_schema": {"type": "object", "fields": {"summary": {"type": "string"}}},
   "bindings": {"summary": {"from_task": "implement"}},
   "input_mapping": {"args": ["/summary"]},
   "output_schema": {"ref": "ReviewResult"},
   "transitions": {"cases": [
     {"when": {"all": [{"compare": {"path": "/verdict", "op": "eq", "value": "pass"}},
                       {"compare": {"path": "/confidence", "op": "ge", "value": 0.9}}]}, "to": ["ship"]},
     {"when": {"all": [{"compare": {"path": "/verdict", "op": "eq", "value": "revise"}},
                       {"compare": {"path": "/confidence", "op": "ge", "value": 0.9}}]}, "to": ["fix"]}],
     "otherwise": ["human_review"]}},
  {"id": "ship", "command": {"kind": "run", "command": ["ship-tool"]}},
  {"id": "fix", "command": {"kind": "run", "command": ["fix-tool"]}},
  {"id": "human_review", "command": {"kind": "run", "command": ["notify-reviewer"]}},
  {"id": "report", "command": {"kind": "run", "command": ["report-tool"]},
   "depends_on": ["ship", "fix", "human_review"]}]}
```

When `review` returns `pass` with confidence 0.95, `task-get` shows:

```text
state: succeeded
route: ship
attempt: review#1
revision: 7
input: summary <- implement (attempt implement#1)
output: {"confidence":0.95,"verdict":"pass"} (from postprocess.stdout.json)
```

- The `input:` line tells you which run of which task the value came from, and the `output:` line tells you where the result came from. A task that ended without a result shows `output: none`, with the failing stage and reason on an `error:` line.
- `fix` and `human_review` end with `skip: branch_not_selected`, and `report` waits only for `ship`, which ran, and succeeds.
- If `review` fails, the three branches and `report` are all skipped, and the cause shows as in `skip: upstream_unavailable (review failed)`.
- `revision` grows each time the task's record is saved. Use it to tell which of two reads of the same task is the later one.
- `confidence` here is a value the task declared in its result type, so branch conditions can use it. The `confidence` returned by a terminal state query is how sure Tasty is about the session's state, which is a different thing.

Tasks without a declared type can live in the same Workspace. A task made with `task-create --depends-on report` runs once `report` finishes. Reading a typed task's result through a placeholder such as `${task.review.output}` is rejected when you create the task, though. To pass a result along, put the receiving task in the graph too and take the value with `bindings`.

## Watching progress

There are two screens for seeing how the work flows. Both look at the same data.

- **Task DAGs** window — `Ctrl+Shift+G`, or the sidebar **Tools** menu. It is for picking one from a list, taking a quick look, and closing it. It has search and a state filter. Pick one and the same area becomes the graph, with zoom in and out, fit to view, direction switching, and the runner pill sitting on the top line next to the back arrow.
- **DAG tab** — a graph that takes up a whole Tab and stays open. Open it with `tasty new tab --pane <ID> --type dag_graph`, or press `Alt+'` on an existing Surface and switch it to **DAG**. It has zoom in and out, fit to view, and direction switching, and clicking a node shows the command, dependencies, elapsed time, exit code, and output.

The lines in the graph differ by relation. A dependency that only sets the order is solid, an input connection (`bindings`) is a teal dash-dot line, and a branch chosen from a result (`transitions`) is a lavender dashed line. A branch that was chosen is drawn thicker, and branches that were not chosen or did not run are faded. When two tasks have both an order dependency and an input connection, only the input connection is drawn. A task skipped because its branch was not chosen is marked **Not selected**, and hovering over the node shows why it was skipped. `task-graph --format dot` draws the same distinctions.

The small picture at the top left of a node is the task kind: a terminal for a shell command, a plug for an internal call, stacked layers for collecting results, a padlock for a wait, and a speech bubble for one Claude or Codex turn. Claude and Codex share the speech bubble; click the node to see which one ran it in the details.

Each list row and the top line of a DAG tab show finished tasks out of the total. When some tasks were skipped, the count follows, as in `· 2 skipped (1 not selected)`; the parenthesis appears only when some of them were skipped because another branch was chosen.

You can run several unrelated graphs in one Workspace. The list groups a chunk connected by dependencies into a single DAG. Attach `--metadata '{"dag":"name"}'` to a task and everything with the same name is grouped together regardless of whether it is connected.

| State | Meaning |
|---|---|
| **Waiting** | A task it depends on has not finished yet |
| **Ready** | It meets the conditions to run and is waiting for the runner |
| **Running** | Running |
| **Succeeded** · **Failed** | Finished |
| **Cancelled** · **Skipped** | A person cancelled it, or something before it failed and it was skipped |
| **Not selected** | Another branch was chosen, so it did not run. This is not a failure |
| **Unknown** | The result is not known. The command ended while Tasty was restarting, so its exit code could not be collected. Later tasks wait until you run it again (`task-retry`) or cancel it. Tasks holding a semaphore or lease behave the same way: if the command is still running, it keeps the resource until it ends and then gives it back |

To see it from a terminal:

```sh
tasty agent dag-list                                   # DAGs across every Workspace
tasty agent task-list --workspace-id 2 --state waiting,ready,running
tasty agent task-get --workspace-id 2 --id "$BUILD"
tasty agent task-graph --workspace-id 2 --format dot   # draw it with Graphviz
```

`--format dot` writes only the graph to standard output, so you can hand it straight to Graphviz (for example `... --format dot | dot -Tsvg > graph.svg`). A cycle warning and the runner status go to standard error. `dag-get --format dot` works the same way.

## Waiting and fixing up

```sh
tasty agent task-await --workspace-id 2 --id "$TEST"             # wait until it finishes
tasty agent task-retry --workspace-id 2 --id "$TEST"             # retry a failed, cancelled or skipped task
tasty agent task-cancel --workspace-id 2 --id "$TEST"
tasty agent task-set-result --workspace-id 2 --id "$MANUAL" --state succeeded
tasty agent task-purge --workspace-id 2 --states succeeded
```

- `task-await` waits up to 10 minutes by default and comes back with a timeout if the task has not finished by then. With `--timeout-ms 0` it waits indefinitely.
- For a typed task, `task-retry` is refused once its fallback has run. If the main task succeeded again, a task that takes either of the two would see two values. To run it again, send it as a new task.
- `task-retry` is refused for a task skipped because its branch was not chosen (**Not selected**). Running it again would skip it again for the same reason.
- Typed tasks do not take `--reset-downstream`. The tasks after them already finished on the earlier run's failure or chosen path. `task-retry` runs only that task again. To rerun the tasks after it, send a new graph.
- `task-set-result` is for reporting that something the runner did not run is done — a check a person does by hand, for example.
  - A typed task gets an attempt ID (`<task ID>#<number>`) each time it runs. Name the attempt with `--attempt-id`. If the task has run again since, the old report is refused instead of finishing the new run.
  - Sending the same report again returns the same answer as the first time. A different result for an attempt that has already finished is refused.
- `task-delete` is refused while another task references it, and tells you the ID of the referencing side. A running task has to be cancelled first.

## Receiving finished work as events

Instead of asking again and again whether a task is done, you can have it come to you.

```sh
tasty events follow --filter 'agent.*'
```

One event per line as JSON, so a shell loop can read it directly.

```sh
tasty events follow --filter 'agent.*' | while read -r line; do
  echo "event: $line"
done
```

- **Save your read position on the receiving side.** Pass the `next_offset` that comes back as the next `--offset` and you carry on from where you stopped. For a single read rather than a loop, use `tasty events fetch --offset <number>`.
- Events live in memory only, and only the most recent ones are kept. If you were away long enough for the ones in between to fall out, you are **not** quietly given the oldest ones instead — you are told how many were missed. That notice is kept out of the event stream, so the `while read` above is not disturbed.
- Restarting Tasty clears the events and starts positions over. If the `epoch` that comes with an answer differs from the one your position came from, that position belongs to the previous Tasty run.
- If the connection drops, `follow` tells you the `--offset` and `--epoch` to reattach with and exits. Run it again with those, and if Tasty restarted in the meantime you are told so and get the new events from their start. With `--reconnect` it does not exit but reconnects every second and carries on.
- What comes out today is **a task finishing** and **a barrier closing**. A task event carries its state together with the run (`attempt_id`), the record version (`revision`) and why it was skipped (`skip`, with the task and state that caused it when an earlier task was the reason). Why something failed is not carried in the event — use `tasty agent task-get` for that.
- Tasty does not keep growing a separate queue for a slow reader. Check for missed events if the reader falls behind the shared recent-event history.

## Concurrency limits and signals

Coordination devices for running several tasks at once come along with it.

| Device | Use |
|---|---|
| Semaphore | Decides how many tasks carrying the same name may run at once. `--concurrency-limit <name>` at task creation is the short form |
| Barrier | Blocks until the set number of signals have gathered. Slot it into the graph as a `wait_barrier` task |
| Lease | Makes something like a file be held by only one holder at a time. It has an expiry, and on a conflict it either fails or returns at once saying it was not acquired — it does not wait |
| Reducer | Merges the results of several tasks into one. Choose between first success only, all of them, JSON merge, or text concatenation |
| Rate limit | Decides how many times per period is allowed, per agent and per metric |

```sh
tasty agent semaphore-create --workspace-id 2 --name build --permits 2
tasty agent barrier-create --workspace-id 2 --name ready --count-required 3
tasty agent lease-acquire --workspace-id 2 --resource file:/tmp/db --holder agent-a --ttl-ms 60000
tasty agent task-reduce --workspace-id 2 --inputs "$A,$B" --strategy all --extract-path /stdout/text
```

Semaphore and barrier names use only lowercase letters, digits, `.`, `_`, and `-`. If a name contains any other character, the command is rejected and tells you which character is the problem. Lease resource names have no such limit.

The full list is in `tasty agent --help`.

<a id="what-to-read-next"></a>

## Keep exploring

- [Driving Tasty from the CLI](cli.md) — The commands agents use in general
- [Claude · Codex](claude-codex.md) — Spawning child agents and receiving completion notifications
- [Hooks · notifications · webhooks](hooks-notifications.md) — Getting notified when a command finishes
