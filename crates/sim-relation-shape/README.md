# sim-relation-shape

Runtime Shape adapters for SIM's pure relational records. Domain declarations
remain data in `sim-relation-core`; this crate resolves their Shape references
through `Cx`, admits cells and rows, and projects inspectable relational record
Shapes without introducing a relation-private matcher or registry.
