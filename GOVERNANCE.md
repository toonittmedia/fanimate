# Governance

How Fanimate is run. This is deliberately light while the project is small and will grow as more people join. Changes to this document go through a pull request like any other change.

## Roles

| Role | Who | Responsibilities |
|---|---|---|
| **Contributors** | Anyone who opens an issue, reviews or sends a pull request | Follow the [Code of Conduct](CODE_OF_CONDUCT.md) and [contribution guide](CONTRIBUTING.md) |
| **Maintainers** | Listed below | Review and merge pull requests, triage issues, cut releases, enforce the Code of Conduct |
| **Steering group** | To be formed during Phase 1 | Sets priorities and the roadmap, resolves disputes |

### Maintainers

- [@emmanuelekopimo](https://github.com/emmanuelekopimo) (project lead)

A contributor becomes a maintainer after sustained, high-quality contributions, by invitation of the existing maintainers.

### Steering group

The product vision calls for a steering group of **working animators, studio technical directors and core developers**, so decisions reflect how animation is really made. Phase 1 aims to recruit its first members: two Moho animators, one Animate user and one studio technical director. Until it exists, the project lead holds its responsibilities.

## How decisions are made

1. **Most decisions happen in pull requests.** A pull request needs approval from one maintainer, and the local checks in [CONTRIBUTING.md](CONTRIBUTING.md#making-a-change) must pass.
2. **Larger changes start as an issue** labelled `proposal`: new features, file format changes, new dependencies with a large footprint, and anything that changes the user interface conventions. Proposals stay open for at least a week for comment.
3. **The roadmap is public.** Priorities are set in the open on the issue tracker, and users can vote on issues with a 👍 reaction.
4. **When there is no agreement**, the maintainers decide; once it exists, the steering group decides.

### Changes that need extra care

- **The `.fnm` file format.** Any change to what is written to disk needs a `proposal` issue, an update to [the specification](docs/spec/fnm.md), and a format version bump if older readers would misread the file. Files written by any released version must always open in later versions.
- **Licensing.** Dependencies must be compatible with MIT / Apache 2.0. Copyleft (GPL, LGPL) dependencies are not accepted in the app or engine.

## Releases

Two release trains, so polish never waits for new features (pain point P16 in the product vision):

| Train | Cadence | Contains |
|---|---|---|
| **Feature release** | About every 4 months | New features that are finished |
| **Stability release** | About every month | Bug fixes and performance work only |

A release is cut with `pnpm release`, which checks, tags and pushes; the tag triggers the cross-platform build and publishes the GitHub release (see [CONTRIBUTING.md](CONTRIBUTING.md#releases)). CI runs only for releases, not on every push.

Phases end at a quality gate, not a date. Version 1.0 ships when every Gate 1 criterion passes. Gate 1 sits at the end of Phase 2 and also includes the frozen scripting API, a body of films and examples and handmade templates (see the [Phase 1 tests](docs/roadmap/phase-1.md#5-gate-1-acceptance-tests) and [docs/roadmap/release-1.0.md](docs/roadmap/release-1.0.md#6-gate-1-criteria-now)).

## Code of Conduct enforcement

Maintainers enforce the [Code of Conduct](CODE_OF_CONDUCT.md). Reports go to the address given there and are handled privately. A maintainer who is involved in a report steps aside from handling it.
