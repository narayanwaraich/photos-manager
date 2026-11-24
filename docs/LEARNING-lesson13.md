# Lesson 13: wait for the thing, not for the clock

Written 2026-11-24, after TASK-1E round 1. One idea, one example from your own project, three commands.

## The idea

A test often has to wait: "start a job, stop it, then look at what it did". There are two ways to wait.

- **Wait on the clock:** `sleep(50 ms)`, then look. The test is really saying "the job surely finishes in 50 ms". That is a guess about the speed of one machine.
- **Wait on the thing:** keep the handle that the job gives you and `await` it, or poll a flag with a deadline. The test continues the moment the job is done, on a fast machine after 2 ms, on a slow one after 400 ms. It never guesses.

If you know React: it is the difference between `await new Promise(r => setTimeout(r, 50))` and `await screen.findByText(...)` (which waits until the text exists, up to a limit).

## Your example

In TASK-1E the test for "progress is throttled" stopped a fake job, slept 50 ms, and read the last event. But the job, before it sends its last event, writes its result to the database and waits for the disk to confirm (a *commit*). On your Mac and on the GitHub Windows machine that took a few milliseconds. On your laptop, once, it took longer than 50 ms, so the test looked at the events too early and saw a progress event where it expected "finished".

Nothing was wrong with the app. The app sends "finished" after the database has the final result, which is the right order. The test was wrong. The agent's panic test in the same file does it correctly: it keeps the `JoinHandle` that `run_tracked_job` returns and `await`s it.

## The habit

1. In a test, never sleep and then assert. Await a handle, or loop `for _ in 0..100 { if done { break } sleep(50 ms) }` and then assert (the loop only sleeps while the thing is not done, and the deadline is generous).
2. A test that could be affected by speed is run many times in a row before it is trusted. If one run in thirty fails, it is a bad test, and you would rather learn it now than from a red CI run.
3. Run `pnpm verify` on both machines. Today it was the second machine that found the problem; that is what it is for.

## Try it (Mac, in the project folder; about 1 minute once the project is built)

```bash
cd ~/Desktop/Projects/photos-manager
for i in $(seq 1 30); do cargo test --manifest-path src-tauri/Cargo.toml test_throttle > /dev/null 2>&1 || { echo "FAILED on run $i"; break; }; done; echo "loop finished"
```

If no line says `FAILED`, all 30 runs passed. On the current `task/1E` branch the test usually passes on the Mac, which is exactly why one passing run proves little.
