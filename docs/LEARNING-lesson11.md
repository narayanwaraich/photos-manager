# Lesson 11: three habits from reviewing TASK-1A

For Narayan. Short. Each point is a real piece of the code in `task/1A`; nothing to run.

## 1. A test that cannot fail is not a test
The agent wrote a property test called `test_proptest_valid_components_never_contain_forbidden`. A property test is like a Jest test that calls your function with hundreds of random inputs. Here the random inputs were built from the pattern `[a-zA-Z0-9_.-]`, which has no `/`, no `\`, no NUL. Then the test asserted that the result contains no `/`, `\` or NUL. The input can never contain them, so the assertion can never fail.

In JavaScript terms:
```js
const names = randomStrings(/[a-z0-9]+/);          // no slash possible
names.forEach(n => expect(clean(n)).not.toContain("/"));   // always passes
```
The fix is to generate the bad characters on purpose and assert the opposite direction: "if the name contains `/`, the function must return an error". Habit from lesson 9: **see a test fail once** (break the code on purpose, watch the test go red, undo the break).

## 2. Match on the type of an error, not on its text
`insert_file` decides "this is a duplicate path" by searching the error message for the words `UNIQUE constraint failed` and `path_key`. If SQLite ever words it differently, or the unique constraint is another one, the code quietly does the wrong thing. It is like checking `err.message.includes("duplicate")` in Node instead of `err.code === "23505"`. SQLx gives the equivalent of a code: `ErrorKind::UniqueViolation`. After that the repository asks "is there really a row with this key?" and only then says "collision".

## 3. Check first, change second
`init_pool` ran the database migrations and *then* checked "was this database made on the other operating system?". A database copied from a Mac to Windows would be changed by a migration before the app refused to open it. Same idea as a Prisma migration running against the wrong database: the safe order is **look, refuse, and only then touch**. The fix moves the check before the migrations, and a new test proves a refused database still has no new tables.

A small bonus from the same review: the code stamped the database with time `0` (1 January 1970). Placeholder values that look real are worse than a missing value, because nobody notices them.
