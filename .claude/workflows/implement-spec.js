export const meta = {
  name: 'implement-spec',
  description: 'Implement a gitt feature spec test-first with fan-out authoring + adversarial coverage verification',
  whenToUse: 'When the user opts into multi-agent orchestration to implement a gitt spec end-to-end (test-first) and wants each acceptance criterion adversarially verified as actually covered.',
  phases: [
    { title: 'Analyze', detail: 'parse the spec into criteria + a test matrix' },
    { title: 'Author tests', detail: 'one agent per module/criterion group writes failing tests' },
    { title: 'Implement', detail: 'implement pure core + thin ports until green' },
    { title: 'Verify coverage', detail: 'adversarially check each criterion is really tested' },
  ],
}

// args: { spec: "specs/log.md" }  (defaults to specs/log.md)
const specPath = (args && args.spec) || 'specs/log.md'

const CRITERIA_SCHEMA = {
  type: 'object',
  required: ['criteria'],
  properties: {
    criteria: {
      type: 'array',
      items: {
        type: 'object',
        required: ['id', 'statement', 'tiers', 'target_module'],
        properties: {
          id: { type: 'string' },
          statement: { type: 'string' },
          tiers: { type: 'array', items: { type: 'string' } },
          target_module: { type: 'string', description: 'src path or tests/ file that should hold the test' },
        },
      },
    },
  },
}

const VERDICT_SCHEMA = {
  type: 'object',
  required: ['id', 'covered', 'evidence'],
  properties: {
    id: { type: 'string' },
    covered: { type: 'boolean', description: 'true only if a real test asserts this criterion' },
    evidence: { type: 'string', description: 'test name + file:line, or why coverage is missing/fake' },
  },
}

phase('Analyze')
const analysis = await agent(
  `Read ${specPath} and CLAUDE.md in this repo. Extract every acceptance criterion into structured form. ` +
  `For each, decide which module/file its test belongs in given the Functional-Core/Imperative-Shell layout ` +
  `(reducer behavior -> src/state/reducer.rs #[cfg(test)]; parsing -> src/parse/*; rendering -> src/ui/*; ` +
  `e2e-tier -> tests/e2e_log.rs). Return the criteria.`,
  { label: 'analyze-spec', schema: CRITERIA_SCHEMA }
)

const criteria = (analysis && analysis.criteria) || []
log(`Parsed ${criteria.length} criteria from ${specPath}`)

// Group criteria by target module so one agent owns each file's failing tests.
const byModule = {}
for (const c of criteria) (byModule[c.target_module] ||= []).push(c)
const groups = Object.entries(byModule).map(([module, items]) => ({ module, items }))

phase('Author tests')
await parallel(groups.map(g => () =>
  agent(
    `Write FAILING tests (TDD, test-first) in ${g.module} for these gitt criteria:\n` +
    g.items.map(c => `- ${c.id} (${c.tiers.join('/')}): ${c.statement}`).join('\n') +
    `\nName each test after its criterion id (e.g. log_05_...). Use fake ports for unit tests and the ` +
    `tui_tester+fixture harness for e2e. Follow CLAUDE.md. Do NOT implement product code yet — only tests. ` +
    `Run \`cargo test\` and confirm they fail for the right reason (compile-or-assert), then report.`,
    { label: `tests:${g.module}`, phase: 'Author tests' }
  )
))

phase('Implement')
await agent(
  `Implement the minimum product code to make all the newly-authored failing tests pass, respecting the ` +
  `Functional-Core/Imperative-Shell rule in CLAUDE.md (logic in domain/parse/state/ui/fuzzy; I/O only behind ` +
  `ports traits; reducer emits Effects, never does I/O). Iterate until \`cargo test\` is fully green and ` +
  `\`cargo clippy -- -D warnings\` is clean. Report the final test summary.`,
  { label: 'implement', phase: 'Implement' }
)

phase('Verify coverage')
const verdicts = await parallel(criteria.map(c => () =>
  agent(
    `Adversarially verify that gitt criterion ${c.id} — "${c.statement}" — is genuinely covered by a real, ` +
    `meaningful test (not a stub, not an over-broad snapshot that would pass even if the behavior broke). ` +
    `Search the tests, read the asserting lines. Default covered=false unless you find a specific assertion. ` +
    `Cite the test name and file:line.`,
    { label: `verify:${c.id}`, phase: 'Verify coverage', schema: VERDICT_SCHEMA }
  )
))

const gaps = verdicts.filter(Boolean).filter(v => !v.covered)
return {
  spec: specPath,
  criteria: criteria.length,
  covered: verdicts.filter(Boolean).filter(v => v.covered).length,
  gaps,
}
