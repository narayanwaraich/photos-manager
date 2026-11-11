# Lesson 8: pipes, exit codes, and a check that cannot fail

For Narayan. Short on purpose.

## 1. What `a | b` does
A **pipe** connects the output of one program to the input of the next, like `a.pipe(b)` with Node streams. `grep -q "text"` means "stay quiet; just tell me whether the text exists". To answer, it stops reading **at the first match**.

## 2. What happens to the program that was still writing
In Node you have seen `Error: write EPIPE`: you wrote to a stream whose reader had already gone. The shell has the same thing, called a broken pipe. In our CI step, `echo "$LOADERS_OUT"` was still writing 30 KB when `grep -q` found `heifload_base` and left. The runner told echo to carry on despite that, so echo printed `write error: Broken pipe` and returned status 1.

## 3. Why that became a red run: `pipefail`
Without `set -o pipefail`, a pipeline's status is the status of its **last** command only. With it (we use it on purpose, so failures are not hidden), any failing command in the pipeline makes the whole pipeline fail. So `if ! echo ... | grep -q heifload` saw a failure although grep had found the text, and printed "does NOT list heifload".

## 4. The dangerous direction
That was the noisy direction: a false alarm. The mirror image is silent. Take `if echo "$out" | grep -q "BAD THING"; then fail; fi`. If the bad thing is near the start of a large output, grep leaves early, the pipeline reports failure, the `if` is false, and the check says "all clear". **A check that can say "fine" without having looked is worse than no check**, because everybody trusts it. That is the pattern the 0F review found in the "no SVG/ImageMagick loader" check and in the "no x265" checks of both audit scripts (in a plain shell it depended on where in the file the text sat).

## 5. How to write it safely
- Do not put an early-exit reader (`grep -q`, `head`) at the end of a pipe: use `[[ "$var" == *text* ]]`, or a here-string `grep -q text <<< "$var"`, or write the output to a file and `grep -q text file`.
- Same idea as `Array.some()` on a list that has already been fetched, instead of cancelling a live download halfway.

## 6. Prove a check can fail, with realistic input
Lesson 7 said: break the input on purpose once. Today's addition: **break it with input of realistic size**. The agent's bad-input tests used four tiny test files; everything fits in the pipe buffer, so the bug hid. It is like a unit test with three list items passing while the list breaks at ten thousand. (My own 0E review missed this: the test string sat at the end of the file, the one place where the old check still works.)

## 7. One-minute experiment (Terminal, nothing is changed)
```
bash -c 'set -o pipefail; big=$(yes "a line of text" | head -c 300000); echo "$big" | grep -q "line"; echo "pipe status: $?"'
bash -c 'set -o pipefail; big=$(yes "a line of text" | head -c 300000); [[ "$big" == *line* ]]; echo "no-pipe status: $?"'
```
The text is there in both cases. Expect `pipe status: 141` (or 1), and `no-pipe status: 0`. Any number other than 0 on the first line is the bug: the check failed although the answer was yes.
