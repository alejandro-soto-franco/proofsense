/-
proofsense-lean: faithful Lean-side extractor.

Loads a compiled subject project's `Environment`, finds a declaration,
delaborates its type, collects its axioms, and emits one JSON object:

  { "decl": string, "type_pp": string, "axioms": [string], "sorry_free": bool }

Pure metaprogramming; deterministic; no network, no LLM.
-/
import Lean

open Lean

namespace Proofsense

/-- Parsed CLI arguments: the fully-qualified decl name and the modules to import. -/
structure Args where
  decl    : String
  imports : Array String
  deriving Inhabited

/-- Split a comma-separated module list, dropping empty fields. -/
def splitMods (s : String) : List String :=
  (s.splitOn ",").filter (· != "")

/-- Consume module tokens until the next `--flag`. -/
partial def takeImports (acc : Array String) : List String → Array String × List String
  | [] => (acc, [])
  | x :: xs =>
    if x.startsWith "--" then (acc, x :: xs)
    else takeImports (acc ++ (splitMods x).toArray) xs

/-- Parse `--decl NAME --imports MOD [MOD ...]`. `--imports` also accepts a
    comma-separated list. Order-independent. -/
partial def parseArgs (args : List String) : Except String Args := do
  let mut decl? : Option String := none
  let mut imports : Array String := #[]
  let mut rest := args
  while !rest.isEmpty do
    match rest with
    | "--decl" :: v :: tl => decl? := some v; rest := tl
    | "--imports" :: tl =>
        let (mods, tl') := takeImports imports tl
        imports := mods; rest := tl'
    | flag :: _ => throw s!"unrecognized or incomplete argument: {flag}"
    | [] => rest := []
  match decl? with
  | none => throw "missing required --decl NAME"
  | some d =>
    if imports.isEmpty then throw "missing required --imports MOD [MOD ...]"
    else pure { decl := d, imports := imports }

/-- Run the extraction against an imported environment and print the JSON line. -/
def emit (env : Environment) (declStr : String) : IO Unit := do
  let declName := declStr.toName
  let some ci := env.find? declName
    | throw <| IO.userError s!"declaration not found in environment: {declStr}"
  let ctx : Core.Context := { fileName := "<proofsense>", fileMap := default }
  let st : Core.State := { env := env }
  let (result, _) ← (do
    let axs ← Lean.collectAxioms declName
    let fmt ← (PrettyPrinter.ppExpr ci.type).run'
    pure (axs, fmt) : CoreM (Array Name × Format)).toIO ctx st
  let (axs, typeFmt) := result
  let typePp := toString typeFmt
  let sorryFree := !axs.contains ``sorryAx
  let axJson := Json.arr (axs.map (fun n => Json.str (toString n)))
  let obj := Json.mkObj [
    ("decl", Json.str declStr),
    ("type_pp", Json.str typePp),
    ("axioms", axJson),
    ("sorry_free", Json.bool sorryFree)]
  IO.println obj.compress

end Proofsense

open Proofsense in
unsafe def main (argv : List String) : IO Unit := do
  match parseArgs argv with
  | .error e =>
      IO.eprintln s!"proofsense-lean: {e}"
      IO.eprintln "usage: proofsense-lean --decl NAME --imports MOD [MOD ...]"
      IO.Process.exit 1
  | .ok args =>
      Lean.enableInitializersExecution
      Lean.initSearchPath (← Lean.findSysroot)
      let imports := args.imports.map (fun m => ({ module := m.toName } : Import))
      -- `loadExts := true` so notation unexpanders (stored in env extensions)
      -- fire during delaboration, giving faithful `+`, `≤`, `∧`, `∈` rather than
      -- raw application forms.
      let env ← Lean.importModules imports (opts := {}) (trustLevel := 1024)
        (loadExts := true)
      try emit env args.decl finally env.freeRegions
