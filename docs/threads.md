# When the system refuses a thread

At the limit on threads (`RLIMIT_NPROC`, `threads-max`, a cgroup's `pids.max`)
`pthread_create` fails with `EAGAIN`. The shell's queues of blocking work
(`crates/workpool`) then keep the job; the caller, which is the shell's main thread for most
of them, never runs it. Tests refuse threads with `testenv::refuse_new_threads`.

| Queue | A job refused its thread | Log line |
| --- | --- | --- |
| `Lane::run` (one queue per service: `Notify.send`, NetworkManager, logind, Hyprland, ...) | stays queued, in order | `rusty-impulse: cannot start a lane thread: …; N queued until one starts` |
| `workpool::long`, no `long` thread running | stays queued | `rusty-impulse: cannot start a pool thread: …; N queued until one starts` |
| `workpool::long`, other `long` threads running | waits for one of them | `rusty-impulse: cannot start another pool thread (…): the job waits for a running one` |
| `workpool::long` called from a `long` job | runs on that job's thread, which is never the main thread | `rusty-impulse: cannot start a pool thread: …; its job runs on its submitter` |
| `workpool::try_long` | where `long` would queue with no thread or run on the submitter: dropped, the caller gets the error (`XMLHttpRequest` ends with `onerror`) | none, the caller's |
| `workpool::run` | stays queued | `rusty-impulse: cannot start a pool worker: …; N queued until one starts` |
| `workpool::after` with a delay, no worker | refused, the caller gets the error | the caller's |
| `Lane::try_run_shared` (a tray menu) | dropped, the caller gets the error | the caller's |
| a child process nothing can wait for | kept, reaped later | `rusty-impulse: cannot start a thread to wait for child …` |

## When the thread is asked for again

Nothing tells a process that the system has a thread to give again: there is no fd, signal
or notification for it. The shell does not poll for it on a timer. It asks again when
something of its own happened that either freed a thread or needs one anyway:

1. **A thread of `workpool` has no work left**: a lane that emptied, a `long` thread with no
   job, the thread that waited for one child. It does not end: it runs the queued jobs
   itself (lanes and `long`, then `run`, then waits for a child nobody waits for). An idle
   `long` thread, which otherwise stays 10 s, is woken for this the moment another queue is
   refused.
2. **A child process was reaped.** That gives back a slot (`RLIMIT_NPROC` counts threads and
   processes together), so the reaper thread asks for every refused thread.
3. **Any queue takes in a job or a child** (`run`, `long`, `after`, `Lane::run`, `reap`,
   `on_exit`): before it does, every refused thread is asked for.
4. **The shell ends** (`flush_for_exit`: `Qt.quit()`, a termination signal, a reload):
   `workpool::finish_held` asks for every refused thread and waits up to 2 s for the jobs
   that waited for one; `notifier::flush` then waits up to 2 s until every `Notify.send`
   made so far, with buttons or without, is on the bus.

A refusal is logged once, when the job is taken in; asking again and being refused again
logs nothing.

## What is not covered

- If none of the four happens, the job stays queued: a shell with no `workpool` thread
  alive that starts no process and takes in no other job has nothing to tell it that
  threads are available again. The next job of any queue, or the exit, ends that.
- Threads that ended outside `workpool` (the D-Bus connections' own, the image pool's,
  PipeWire's) do not ask. Their end frees a thread all the same, and the next event of the
  list above finds it.
- The four workers of `workpool::run` never end, so one that is idle does not take over
  another queue.
- A job a queue got its thread for at the very end of the process is not waited for, as
  before: only jobs that had been refused a thread, and notifications.
