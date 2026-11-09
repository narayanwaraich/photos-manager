# Lesson 6: a check that always fails, a check that never fails (the x265 audit)

**Where to look:** the failed audit step of run 42576054985 (Actions, "Build & Test Windows x64 libvips"), and `reports/REPORT-0E.md`, the part about `libvips-42.dll`.

## 1. What happened
The build produced the library zip we wanted. Then our own safety check stopped it: it searched every DLL for the text `x265` (the GPL video encoder we must not ship) and found it in `libvips-42.dll`. The agent did the right thing next: it ran the same check on **upstream's official zip**, which has no HEVC code at all, and got the same hit. The text is the *name of an option* ("x265" is one of the encoder choices libvips lists in its menu of choices), not the encoder itself. Like finding the word `bomb` in a dictionary and concluding the book is dangerous.

## 2. The two ways a check can be wrong
| | What it does | Danger |
|---|---|---|
| False positive | fails when nothing is wrong | people learn to ignore or switch the check off |
| False negative | passes when something is wrong | you ship the problem and feel safe |
A good check must be tested against **a known good input** (it must pass: here, upstream's zip) and **a known bad input** (it must fail: here, a zip with a fake `libx265.dll`). JavaScript parallel: a unit test you have never seen fail proves nothing; a lint rule that fires on every file gets disabled.

## 3. Narrow carefully
The quick fix is to stop looking in `libvips-42.dll`. But that is exactly where a real mistake would show up too. Better: keep looking, but for strings only a real x265 would contain (`libx265`, `x265_encoder_open`). Make the check more specific, not smaller.

## 4. Why it is a script file now
Logic hidden inside a YAML `run: |` block can only be tested by pushing to GitHub and waiting 16 minutes. As `deps/win64-vips/audit-zip.sh` you run it on your Mac in two seconds, on good and bad zips.

## Try it (10 min, Terminal)
1. Write down one false positive and one false negative you have met in JavaScript tooling (an ESLint rule that fired on correct code; a test that stayed green while the feature was broken). What would you have needed to notice?
2. Think: if the audit had searched only for the word `rav1e`, would it have flagged `libvips-42.dll`? (Yes: the same list contains `rav1e`.) What does that tell you about searching for short names?
3. In `deps/win64-vips/audit-zip.sh` (once it exists), find the rule with the comment explaining the allowed `x265` string and read the evidence it cites.
