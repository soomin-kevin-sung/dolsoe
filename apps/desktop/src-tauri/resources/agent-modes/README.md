# Agent mode prompt packages

Each runnable agent mode owns a directory under this folder.

```text
agent-modes/
  chat/
    system.md
  react/
    system.md
    decision.gbnf
```

`system.md` contains mode-specific system instructions. A `*.gbnf` file constrains
the model output for one model stage; it does not replace the system prompt or
validate the meaning of generated values.

The Rust agent-mode registry binds every stage to its instructions, optional
grammar, revision, and execution policy. Do not load prompt files by a path
derived from user input.

Multi-stage modes keep one instruction and optional grammar file per stage:

```text
plan-and-solve/
  planner.md
  planner.gbnf
  solver.md
  decision.gbnf
```

Plan & Solve remains unavailable until its planner-to-solver transition and
recovery policy are implemented. Prompt assets alone must not make a mode
selectable.

## GBNF lifecycle

1. The mode registry selects the grammar for the current model stage.
2. The grammar is copied into `SubmitRequest.output_grammar`.
3. The native runtime passes it to the llama.cpp grammar sampler.
4. Invalid next tokens are excluded while the model is generating.
5. The completed output is parsed and semantically validated by Rust.

Keep parser validation even when a grammar is present. Grammar-constrained
output can still name a nonexistent path or request an invalid operation.
