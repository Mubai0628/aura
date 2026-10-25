//! Schema text the signed built-in tool set was first released with. That set is pinned by its
//! source hash and materialized into stores at its single release revision, so changing this text
//! would change the signed set; the agent release gives the model its own descriptions.

/// The input schema description of the run-information query in the signed set.
pub(crate) const AGENT_ACTIVITY_QUERY_INPUT: &str = "\
S04.5 B/R4: the **model-facing** input of the run-information query.\n\n\
WHY IT LIVES HERE AND NOT IN THE TOOL CONTRACT\n\
The query's own contract already lives in this crate (`ActivityQueryRequestV1`), and this is the\n\
same question asked by a different caller. Putting it in `aura-tool-contract` would make that\n\
low-level crate depend on this one and invert the dependency direction the design fixes; here it\n\
sits beside the wire shape it mirrors, so the two cannot drift.\n\n\
WHY IT IS THIS SMALL, AND WHAT IT DELIBERATELY CANNOT SAY\n\
The model may name **what it is looking for** and nothing else. It cannot name a session, a\n\
workspace or a grant: those are resolved from the step's own authenticated position exactly as\n\
the IPC route resolves them. A model that could name its own session would widen its own\n\
visibility by asking, which is the defect R4 names. `target` is a **lookup hint, not a grant** -\n\
it names an already-verified object the caller is asking about, and it can only ever match a fact\n\
a real publisher recorded.";
