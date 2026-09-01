# sim-relation-shape

In one line: Shape projection and runtime admission for SIM relational records.

## What it gives you

Runtime adapters that resolve relational Shape references through Cx, admit typed cells and rows, and expose inspectable record Shapes. Relational declarations remain ordinary data rather than growing a private matcher or registry. Relation values use the same checking and browsing machinery as the rest of SIM. Domain mismatches are reported at the shared Shape boundary. Tools can inspect row contracts without learning provider details. This crate connects pure relation records to sim-shape. It owns projection and admission only; planning, storage, and provider execution remain separate. The contract keeps inputs, outputs, limits, and refusal cases explicit, so callers can compose the capability without acquiring unrelated host, transport, or product authority. Stable records make the result suitable for tests, inspection, and deterministic integration.

## Why you will be glad

- The public contract makes supported behavior, limits, and typed failures visible before integration.
- One owning crate prevents neighboring libraries from growing competing copies of the same policy.
- Deterministic records and checked tests keep adapters reviewable when implementations evolve.

## Where it fits

Within SIM, sim-relation-shape owns only the focused contract described above. Adjacent runtime libraries, platform adapters, codecs, and user surfaces can build around it while retaining their own policy. That boundary keeps the kernel small, avoids competing implementations, and lets this capability evolve without forcing unrelated components to change.
