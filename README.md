# xmip-core-contract-yaml

YAML content contract: well-formed always, holding the keys and types a layout names when a Location binds one. A technology of [xmip-core-contract](https://github.com/IlleNilsson/xmip-core-contract).

Every text is held to a budget before it is built (`budget`): the copies its
aliases make may weigh at most 100 000 nodes and scalar bytes, and it may nest
at most 64 levels, an alias's copy counted as deep as what it copies. Aliases
are counted rather than refused, because anchors and merge keys are ordinary
YAML; billion laughs is refused at the alias that crosses the budget, and deep
nesting before the parser's recursion can overflow the stack.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
