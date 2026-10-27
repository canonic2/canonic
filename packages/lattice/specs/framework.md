# Lattice framework

## Purpose and status

Lattice provides efficient primitives and tools for application behavior across
languages and native platforms. JavaScript is one implementation of the
framework. Swift, Kotlin, Windows and Linux are also in scope. Windows and Linux
identify platform targets; their implementation languages and UI integrations
remain undecided.

**Status:** agreed product direction. Only the JavaScript implementation exists.
Native libraries, a definition compiler and cross-platform conformance support
remain to design and implement. This page establishes their boundaries, not
their public APIs or delivery schedule.

## Native primitives

Each supported language or platform provides appropriate Lattice primitives,
functions and helpers that applications can use directly. Definition generation
is optional; applications do not need a definition file to use native primitives.

Implementations may differ substantially in API, ownership, data structures,
concurrency, scheduling and integration. They follow the host's language and
lifecycle conventions. Shared intent does not require matching method names,
JavaScript object semantics, DOM concepts or one universal runtime.

Native UI owns presentation and interaction. Lattice supplies behavior and
explicit integration boundaries. Native controls, Web Components and platform
UI frameworks remain responsible for their own presentation. No shared renderer
or cross-platform widget API is established by this direction.

Application rules stay separate from presentation and external I/O. Mutable facts
have explicit owners, dependencies are explicit, and resources have defined
lifetimes. Platform implementations choose the least costly mechanism that meets
their observable contract for the intended workload. Selectivity, caching and
scheduling must justify their own overhead.

## Shared definitions and generation

Lattice's direction includes shared behavior definitions and deterministic code
generation into target-language code. YAML is an agreed authoring format for
declarative behavior; its schema and syntax remain to design. It is compiler
input, with no requirement to ship or interpret YAML during application updates.

Generated code can use the target's Lattice primitives and native facilities.
Generation does not eliminate runtime state or lifecycle machinery. Shared
mechanisms and specialized generated code must be evaluated for total cost,
including code size, startup, allocations, retained memory and execution work.

Definitions describe portable behavior with explicit semantics. Custom logic
needs explicit typed extension boundaries; arbitrary application code is not
assumed to translate automatically between languages. Generated and handwritten
code have separate ownership so regeneration preserves application code.

Native UI can be authored by developers or agents against those boundaries.
Agents are consumers of the contracts; deterministic generation owns repeatable
translation and validation.

## Capability scope and deferred decisions

Forms and validation, network operations, query/cache behavior and data
persistence are in scope for future design. Integration with existing OpenAPI
and GraphQL definitions is also part of the direction. These capabilities must
remain separable so an application loads only the behavior it needs.

This page defines no form schema, validator API, query key, cache policy, storage
engine, networking API or generated operation shape. It makes no Yup/Joi
compatibility promise. Each capability needs its own contract, platform design
and acceptance evidence before its API is adopted.

The existing JavaScript rules that keep application persistence and transport
outside its current primitives retain their scope. They do not exclude future,
separately designed Lattice capabilities in these areas.

## Specification ownership

Shared framework direction belongs here. Future shared capability contracts
will own portable behavior and state which semantics apply across targets.
Language/platform specifications own their APIs, host constraints, architecture,
tooling and acceptance evidence. Differences must be explicit rather than
inferred from one implementation.

The [JavaScript specifications](javascript/README.md) own the current stores,
collections, Web Component helpers, binding conventions and workload roadmap.
Its `framework/` folder is the JavaScript authoring guide. It does not own the
whole framework's authoring model. JavaScript's unchanged-source delivery,
JSDoc and browser requirements do not constrain native toolchains.

Create additional language/platform specification areas when design work begins;
do not populate them with speculative APIs or empty implementations. Runtime
source layout, package names and existing import paths are unchanged by this
specification separation.

## Acceptance boundaries

- Identify available capabilities separately for each language and platform.
  An agreed target does not establish implementation or host support.
- For generated portable behavior, define common behavioral fixtures and run
  them against each target. Platform-specific behavior has separate fixtures.
- Document unsupported semantics and explicit extension points. Do not silently
  weaken a definition when a target cannot implement it.
- Establish performance on each target with representative end-to-end workloads,
  work counts and native baselines. Evidence from JavaScript does not transfer
  automatically, and no universally fastest implementation is claimed.
- Preserve the existing JavaScript contracts and roadmap during this scope
  change. Native APIs, compiler design and forms/query/data APIs require later
  decisions; this document does not authorize their implementation.
