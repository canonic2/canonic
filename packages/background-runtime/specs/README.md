# Background runtime specifications

These specifications define `canonic-background-runtime`: the shared mechanics of a
per-user background manager, the guarantees products can rely on, and the
boundary between library and product responsibilities. They are the contract
for implementation; reader-facing usage belongs in [docs](../docs/README.md).

Start with the [overview](overview.md) for the workflow and vocabulary, then
read the topic that owns the behavior being changed. Requirements describe
intended behavior. A topic's implementation gaps describe where the code does
not yet meet that contract; they do not weaken it.

| Topic | Owns |
| --- | --- |
| [Overview](overview.md) | Purpose, workflow, vocabulary, responsibilities and status |
| [Architecture](architecture.md) | Core, driver, callbacks and ownership |
| [Endpoints](endpoints.md) | Private roots, peer credentials, endpoint identity |
| [Single instance and start](startup.md) | Leases, readiness, detached and registered launch |
| [Framing and versions](protocol.md) | Bytes, JSON envelopes, controls, capabilities, ambiguity |
| [Updates and manager replacement](updates.md) | Release detection, lease-ordered replacement and product state recovery |
| [Failure recovery](recovery.md) | Authorized request replay, owner death and explicit force-stop |
| [Admission and lifetime](lifetime.md) | Atomic fencing, policies, holds, observers, events |
| [Limits](limits.md) | Size, count and time bounds, validation and deadline ownership |
| [Security](security.md) | Protected properties, trust boundary, threats and non-goals |
| [Platforms](platforms.md) | Implemented adapters and explicit gaps |
| [Verification](verification.md) | Requirement-to-test map and ownership |

Each requirement has one owning topic; related topics link to it. Numeric
bounds belong to [Limits](limits.md). [Verification](verification.md) maps
contracts to existing tests and names missing evidence without recording check
runs. [Platforms](platforms.md) distinguishes implementation from host acceptance.
