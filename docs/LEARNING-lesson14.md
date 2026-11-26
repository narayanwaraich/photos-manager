# Lesson 14: an empty folder is a complete walk

Written 2026-11-26, after TASK-1E was merged. One idea, one example from your own project, two commands.

## The idea

The scanner has a rule from TASK-1D: **nothing is marked `missing` unless the walk completed.** That rule protects you from a crash halfway through. It does not protect you from a walk that completes on the wrong thing.

Think about an external drive. You unplug it. On a Mac the folder `/Volumes/My Passport` normally disappears with it, and the app notices ("not reachable"). But sometimes something leaves an empty folder with the same name behind, or Windows gives the drive a different letter and another drive takes the old one. The app now looks at a folder that exists, walks it, finds nothing, and the walk is *complete*. By the 1D rule, every photo of that root is then "not seen in this scan", so all of them are marked `missing`. Nothing was deleted, but the catalogue now says your whole library is gone.

The general lesson: **"I found nothing" and "there is nothing" are different statements.** Before a program acts on an absence, it must check that it was looking in the right place.

## What TASK-1F does about it

The fix is not "be more careful with the walk". It is a second question asked before the walk: *is the disk under this path the same disk I stored?* Every volume has an identity number (the `fs_uuid` column in `volumes`). Before a scan, the app asks the operating system for the identity of the volume under the root's path and compares it with the stored one. A different identity means the root is treated as **offline** and the scan is refused. No file row is touched, so there is nothing to undo when the real drive comes back.

This is the same habit as lesson 13 (test the thing, not a guess) turned around: act on the thing, not on the name of the thing. A path is a name. The volume identity is the thing.

If you know Git: it is the reason `git push` asks which *remote* you mean and a branch name alone is not enough. Two repositories can both have a branch called `main`.

## Try it (Mac, any folder; 1 minute)

```bash
diskutil info / | grep -i "volume uuid"
ls /Volumes
```

The first line prints the identity of your system disk. Plug in your USB drive (the one you used for the 1C tests), run `ls /Volumes` again and then `diskutil info "/Volumes/My Passport" | grep -i "volume uuid"` (use the name that `ls /Volumes` shows). That second number is what the app stores for that drive. It stays the same when you unplug and replug the drive, and when the drive gets another name. It is different from the one of your system disk.

## The habit

1. When a program is about to treat *absence* as a fact ("missing", "deleted", "empty"), ask what proves it looked at the right place.
2. Prefer an identity (a UUID, an inode, a hash) to a name (a path, a label) wherever both exist.
3. When you review evidence, the same question applies to you: the file said "round 2" but the test list said round 1 (lesson 13's example). A name that is not checked against the content proves nothing.
