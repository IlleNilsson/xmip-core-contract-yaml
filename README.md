# xmip-core-contract-yaml

YAML content contract: well-formed always, holding the keys and types a layout names when a Location binds one. A technology of [xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Every text is held to a budget before it is built (`budget`): the copies its
aliases make may weigh at most 100 000 nodes and scalar bytes, and it may nest
at most 64 levels, an alias's copy counted as deep as what it copies. Aliases
are counted rather than refused, because anchors and merge keys are ordinary
YAML; billion laughs is refused at the alias that crosses the budget, and deep
nesting before the parser's recursion can overflow the stack.

## The declaration

What a Location naming this contract is configured with is two `const`s in
its `declaration` module, `src/declaration.rs`, beside the code, which its
`ContractFactory` reads (ADR-0064, amendment 2026-11-26):
`declaration::SETTINGS`, `reference`, the path of the layout documents are
held to, and `NAME`, `yaml`, the name a Validate step's `format` names it
by. Built with its `declaration` feature alone (`--no-default-features
--features declaration`), the crate is those and nothing more: its code and
every crate the code is built on are behind the default feature. So the
library the surfaces load can carry it without the code, for the language
server and the desktop to suggest and check a Location's contract settings
though no node in their process loaded the technology, once the gathering
crate of open problem 48 links it. `test/Declaration.Test.ps1` in the estate
holds that the declaration builds nothing the contract capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
