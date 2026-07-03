# sim-shape

In one line: it is the single component that decides whether a piece of data fits a described pattern, and tells you exactly what it found inside.

## What it gives you

sim-shape gives SIM one shared way to describe the form a value should take and then check any value against that description. A shape can say "any number", "exactly this word", "a list of these", or a named class, and it can be combined with and, or, and not to build richer descriptions. When a value is checked, the shape does two jobs at once: it reports whether the value matches, and it captures the interesting parts by name so the rest of the system can use them. The same descriptions also compare against each other, so you can tell when one pattern is stricter than another. Because one engine does all this, matching stays consistent everywhere it is used.

## Why you will be glad

- One matching engine serves parsing, checking, binding, and picking the right function, so behavior is the same across the whole system.
- Every check explains itself, reporting what matched and what it captured instead of a bare yes or no.
- Patterns can be compared, so you can tell when one description fully covers another and catch overlaps before they cause trouble.

## Where it fits

sim-shape sits just above the SIM kernel, which owns only the open shape idea and leaves the concrete work to this crate. Whenever another part of SIM needs to read input, confirm a value is well formed, bind names, choose among several definitions, or match a grammar, it reaches for these shapes rather than inventing its own rules. That keeps one trusted answer to the question "does this fit, and what is inside it" across the constellation.
