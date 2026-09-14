# Examples

Real documents, every one of them parsed by `irena-core`'s tests and validated against
the schemas under [`../schemas`](../schemas) with `xmllint`, so they cannot drift from
what the code accepts. The `irena` CLI tests found chains from them.

| File | Shows |
|---|---|
| [`genesis-three-channels.xml`](genesis-three-channels.xml) | A company founded with `shareholders` (share register, collective), `board` (roster of three, collective, weighted) and `ceo` (roster of one, individual) — three ways to decide, one implementation |
| [`genesis-single-member.xml`](genesis-single-member.xml) | A one-holder company whose only channel takes the share register in **individual** mode: the sole member signs alone, with no "sole director" type anywhere |
| [`channels-shareholders-only.xml`](channels-shareholders-only.xml) | The minimum: one collective channel over the register — the classic shareholders' vote as a configuration |
| [`channels-weighted-committee.xml`](channels-weighted-committee.xml) | A weighted committee with a two-thirds threshold beside the shareholders; no committee type needed |
| [`channels-ceo-abolished.xml`](channels-ceo-abolished.xml) | A channel-set amendment the `ceo` channel **may** execute alone: it abolishes itself |
| [`channels-ceo-thins-board.xml`](channels-ceo-thins-board.xml) | A channel-set amendment the `ceo` channel **may not** execute alone: it changes a board its actor sits on — refused by the self-demotion rule |
| [`identities-rotate-chen.xml`](identities-rotate-chen.xml) | An identities amendment: chen's key rotated, quinn registered. One record, and every channel chen sits on sees the new key |
| [`authorisation-two-secretaries.xml`](authorisation-two-secretaries.xml) | An authorisation amendment: a second `company` signer, and a `governance`-only signer |

Holders and roster members carry an id only. Keys live once, under `<identities>`,
where a person may also carry an opaque `document-id` (a national id or passport
number). The keys are Ed25519 public keys derived from fixed seeds (`[1;32]` for
alice, `[2;32]` for bob, `[3;32]` for chen after rotation, `[4;32]` for chen, `[5;32]`
for okafor, `[7;32]` for ada, `[8;32]` for quinn, `[9;32]` for jane) so the
documentation drill can sign with them. A real company registers its people's own
keys.

How the pieces fit is in [`../governance.md`](../governance.md); the exact rules are in
[`../IRENA_V1.md`](../IRENA_V1.md).
