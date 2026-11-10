# Lesson 7: fail closed, prove a check can fail, and what a RAW file really is

For Narayan. Short on purpose.

## 1. Fail open versus fail closed
A **fail-open** check is one that quietly lets things through when it cannot do its job. Our CI download step did that: if the private download failed, it used the old library, printed a green tick, and HEIC would never have been tested again. A **fail-closed** check goes red when it cannot do its job, so a person notices. Rule of thumb: security and "is the right thing installed" checks fail closed. The one exception we kept: pull requests from other people's forks have no secrets, so only they may fall back, and they say so in the report (`heic_decode_verified: false`, note "no deps token (fork)").

## 2. A check you have never seen fail proves nothing
The agent proved the new step by pushing a throwaway branch with a wrong release name and showing that the run went red at exactly that step, then deleted the branch. Do this for any new guard: break the input on purpose once. (Same idea as Lesson 6, where we tested the audit script on known-bad inputs.)

## 3. Least privilege for tokens
The first CI token had no permission to read releases (HTTP 403). The shortcut, copying your personal login token into the secret, would have given CI access to all your repositories. The right fix was a token that can only read one repo. Rules now in AGENTS.md: agents never create, change or delete secrets, tokens, repositories, releases or settings. When one is needed they stop and tell you what to do.

## 4. What the "RAW" badge on an iPhone photo means
Your iPhone 12 Pro can save **Apple ProRAW**: a DNG file (26 MB, 12 megapixels) that keeps the sensor data so you can edit exposure and colour later. A JPEG or HEIC is already processed and compressed (1 to 3 MB). DNG files normally also contain a ready-made **JPEG preview**; yours has a full-size one (4032x3024, 3.8 MB). So the photo manager can show your RAW photos by extracting the preview (ExifTool does that) and thumbnailing it, without decoding the RAW data. That is D15 Tier 1.
**Why AirDrop gave you a JPEG:** Photos converts a RAW photo to a JPEG when it shares it, to be compatible. Save to Files first (or Export Unmodified Original on the Mac) and the file stays a DNG.
