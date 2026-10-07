# Dependency licensing and commercial use

Blitz's own Rust code is generally `MIT OR Apache-2.0` (`stylo_taffy` also offers
`MPL-2.0`). The dependency graph is **not entirely permissively licensed**:
Servo/Stylo and several other dependencies require MPL-2.0. MPL permits use in
commercial, closed-source applications, including static linking, but imposes
file-level source-disclosure obligations when distributing the covered code.

This is a technical license review, not legal advice or a guarantee of compliance
for a particular product. License compliance and patent/codec rights are separate
questions.

## Automated policy

The `Dependency licenses` CI job runs:

```sh
cargo deny --workspace --all-features --locked check licenses
```

Install the tool locally with `cargo install cargo-deny --locked`. `deny.toml`
checks all workspace members, including private examples, build dependencies and
dev dependencies. All features are enabled and there is no target filter, so
Windows, Apple, Linux, Android and Wasm dependencies are checked together without
having to build those targets. Only the license check runs; this is not an
advisory, duplicate-dependency or dependency-source audit.

All license expressions must be satisfied by the explicit allowlist, which allows
permissive licenses and MPL-2.0 globally. Unknown/unlicensed packages and licenses
outside this list (including GPL, AGPL and LGPL) fail. Do not add a license merely
to make CI green: review its terms and the dependency's actual license files first.

## Review of the current Cargo dependency graph

The initial review inspected 990 packages reported by Cargo metadata for the
locked, all-features workspace. cargo-deny 0.20.2 checked the 958 packages actually
reachable with those features and reported zero errors or warnings after adding
the missing workspace license declaration to `wasm_hello`. The other 32 packages
are inactive optional dependencies; their declared expressions also have
permissive license choices.

| License family | Commercial/proprietary use | Main distribution obligations |
| --- | --- | --- |
| MIT, ISC, BSD-2-Clause, BSD-3-Clause, NCSA | Permitted; no source-disclosure requirement | Preserve required copyright, permission and disclaimer notices; respect non-endorsement clauses. |
| Apache-2.0, Apache-2.0 WITH LLVM-exception | Permitted; no source-disclosure requirement | Include the license, retain required notices/NOTICE content and identify modifications where required. Apache includes a patent grant and patent-termination provisions; the LLVM exception relaxes some object-code notice requirements. |
| BSL-1.0 (Boost), Zlib | Permitted; no source-disclosure requirement | Follow their notice and altered-source requirements. `BSL-1.0` means the **Boost Software License**, not the Business Source License. |
| Unicode-3.0, Unicode-DFS-2016 | Permitted, including sale of software/data | Preserve copyright and permission notices in copies or associated documentation; respect restrictions on promotional use of names. |
| CC0-1.0 | Public-domain dedication with fallback license | No copyleft obligation; it does not grant patent or trademark rights. |
| MPL-2.0 | Permitted, including proprietary larger works | Make the corresponding covered source, including modifications, available under MPL; inform recipients how to obtain it and preserve notices. |

Expressions using `OR` offer a choice; expressions using `AND` require all listed
licenses. For example, `r-efi` offers MIT/Apache alternatives to LGPL and
`stylo_taffy` offers MIT/Apache alternatives to MPL. These do **not** require a
GNU-license allowance. `encoding_rs` requires BSD-3-Clause in addition to a
MIT/Apache choice, `unicode-ident` requires Unicode-3.0 in addition to a
MIT/Apache choice, and `libfuzzer-sys` requires NCSA in addition to a MIT/Apache
choice. No dependency requires GPL, AGPL or LGPL to satisfy its declared license
expression under this policy.

### MPL-2.0 dependencies

MPL-2.0 is allowed globally. The current MPL-only dependencies are:

- Style/Servo stack: `app_units`, `cssparser`, `cssparser-macros`, `dtoa-short`,
  `selectors`, `stylo`, `stylo_atoms`, `stylo_derive`, `stylo_dom`,
  `stylo_static_prefs`, `stylo_traits`, `to_shmem`, `to_shmem_derive`, `uluru`.
- Other existing dependencies: `colored`, `mp4parse`, `nucleo`, `nucleo-matcher`,
  `option-ext`.

Under [MPL sections 3.1–3.3](https://www.mozilla.org/en-US/MPL/2.0/), a proprietary
application can remain closed source provided its separate files are not covered
by MPL. Static linking does not, by itself, require publishing the entire
application. Distributing MPL-covered executables does require making the covered
source available, **even if unmodified**, and telling recipients how to obtain it.
Changes to MPL-covered files must remain available under MPL. Copying MPL code
into an application's files can make those files subject to MPL. See
[Mozilla's FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/), especially questions
8–12, for the distinction between covered files and a larger work.

## Product-release checklist and limits

- Generate third-party notices for the **actual** shipped dependency graph,
  features and targets. A green cargo-deny check does not generate notices or
  satisfy their delivery requirements.
- Include the chosen license texts and required attribution/NOTICE material.
- For shipped MPL components, provide their exact corresponding source (and any
  modifications) and a clear source-access notice. Do not rely on a link to a
  moving branch; ensure source remains reasonably available to recipients.
- Audit bundled assets and native libraries separately. Cargo metadata does not
  describe system libraries, code downloaded by build scripts (for example the
  optional Skia backend), fonts, or application content.
- The bundled Mozilla bullet font and example DejaVu Sans fonts are outside the
  Cargo license check, as are the MPL-licensed stylesheets in `blitz-dom/assets`.
  DejaVu's Bitstream Vera/Arev terms permit inclusion in a
  commercial software package but require notices, restrict certain modified
  font names, and prohibit selling the fonts alone. See
  [DejaVu's license](https://github.com/dejavu-fonts/dejavu-fonts/blob/master/LICENSE).
  Preserve font notices and verify provenance/licensing for redistributed font
  assets before release. The bundled bullet font identifies Mozilla and Mats
  Palmgren but does not embed a license notice. Mozilla's
  [current font source](https://searchfox.org/firefox-main/source/layout/style/res/Mozilla_Bullet.bf)
  declares the SIL Open Font License; confirm this covers the bundled version
  and include its notices before redistributing it.

Strictly permissive-only use is **not** possible with the current Stylo-based
stack. Commercial/proprietary use is supported by the reviewed Cargo licenses
provided the applicable conditions above are met; a product-specific release
audit is still necessary.
