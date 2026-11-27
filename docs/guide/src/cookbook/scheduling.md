# Scheduling

**The problem.** Five jobs must run on a single machine, one at a time. Each job takes a
certain time and cannot start before its release time, and some jobs must wait until others are
finished: you cannot test before you build. When should each job start so that everything is
done as early as possible?

| Job    | Duration | Earliest start | Must wait for |
|--------|----------|----------------|---------------|
| design | 3        | 0              |               |
| build  | 5        | 0              | design        |
| test   | 2        | 0              | build         |
| docs   | 2        | 1              |               |
| ship   | 1        | 0              | test, docs    |

**The model.** A struct describes each job. The unknowns are the start times, bounded by a
*horizon*: running every job back to back can never take longer than the sum of all durations.
Three kinds of requirements follow:

1. a job starts no earlier than its release time;
2. two jobs on the same machine do not overlap: one of them ends before the other starts;
3. a job that must wait starts after the other one ends.

The goal, the *makespan*, is the time the last job ends, and we minimize it:

```rsat
struct Job { name: string, duration: int, release: int }

var jobs = [
    Job("design", 3, 0), Job("build", 5, 0), Job("test", 2, 0),
    Job("docs", 2, 1), Job("ship", 1, 0),
]
var after = [[0, 1], [1, 2], [2, 4], [3, 4]]      // [a, b]: b starts after a ends
var n = len(jobs)
var horizon = sum(job.duration for job in jobs) + 1
var start: [int[0..=horizon]; n]

fn end(start: int, job: Job) -> int {
    return start + job.duration
}

for i in 0..n {
    require start[i] >= jobs[i].release
    for j in i + 1..n {
        // One machine: jobs i and j do not overlap.
        require end(start[i], jobs[i]) <= start[j] or end(start[j], jobs[j]) <= start[i]
    }
}
for pair in after {
    require end(start[pair[0]], jobs[pair[0]]) <= start[pair[1]]
}

var makespan = max(end(start[i], jobs[i]) for i in 0..n)
solve start minimize makespan {
    for i in 0..n {
        print "{jobs[i].name}: {start[i]}–{end(start[i], jobs[i])}"
    }
    print "everything done at {makespan}"
}
```

```text
design: 0–3
build: 3–8
test: 10–12
docs: 8–10
ship: 12–13
everything done at 13
```

Some things worth noticing:

- **Known data, unknown decisions.** The jobs, with their names and durations, are known
  structs. The function `end` works on a known job and an unknown start time alike.
- **"Or" constraints.** The no-overlap rule says "i before j, or j before i". The solver picks
  the order; that choice is what makes scheduling hard.
- **The makespan is a `max`.** Minimizing the largest end time pulls every job as early as the
  rules allow. With 13 time units of work in total, 13 is clearly optimal here.

To schedule on several machines, add an unknown machine per job (`int[0..machines]`), and only
require "no overlap" for jobs on the same machine:
`require machine[i] == machine[j] implies (… or …)`.
