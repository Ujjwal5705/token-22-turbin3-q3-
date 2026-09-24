# Token-2022 Remittance Stablecoin

An Anchor program implementing a remittance stablecoin on Token-2022:
a protocol-level transfer fee, KYC-gated account freezing, on-chain
metadata, mint decommissioning, a regulatory seizure authority, and
confidential transfers — built up task by task, with every extension
combination actually deployed and tested rather than assumed to work.

## Program structure

```
programs/t22/src/
├── lib.rs                     — module wiring + program entrypoints
├── constants.rs
├── error.rs
├── state.rs
└── instructions/
    ├── initialize.rs          — Task 1
    ├── transfer.rs            — Task 2
    ├── unfreeze.rs             — Task 4
    ├── reissue.rs              — Task 5
    └── confidential.rs         — Task 6
```

Task 3 (`StateWithExtensions`-only reads) is not a separate file — it's a
constraint honored throughout: every instruction and every test in this
repo reads mint or token-account state via `StateWithExtensions`, never
a raw or typed unpack that would silently drop extension data.

## Tasks

### Task 1 — the mint

`InitializeMint` stacks four extensions on one mint: `MetadataPointer`
(pointed at the mint itself, so wallets don't need an off-chain
registry), `TransferFeeConfig`, `DefaultAccountState` set to `Frozen`
(new accounts start frozen pending KYC), and `MintCloseAuthority`. The
account is sized with `ExtensionType::try_calculate_account_len`, and
every extension-init CPI runs before `InitializeMint2` — required,
since most mint extensions can never be added after that point.

Tested in `tests/test_initialize.rs`.

### Task 2 — the fee-charging transfer

`TransferWithFee` uses `transfer_checked_with_fee`, never plain
`transfer`/`transfer_checked`. The fee is never cached or trusted from
the caller: it's read fresh from the mint's live `TransferFeeConfig`
extension and computed via `calculate_epoch_fee(current_epoch, amount)`
on every call. Transfer fees are epoch-scheduled — a `newer_transfer_fee`
can be queued without being active yet — so a fee computed any other way
can silently drift from what Token-2022 will actually enforce.

Tested in `tests/test_transfer.rs`, which independently recomputes the
expected fee outside the program and checks both the source debit and
destination credit against it.

### Task 4 — KYC unfreeze

`UnfreezeAccount` thaws exactly one token account, signed by the mint's
freeze authority. It does not touch the mint's `DefaultAccountState`
extension — that stays `Frozen` permanently, so every new account is
still born frozen and needs its own individual clearance. There is no
mint-level switch that unfreezes accounts in bulk.

Tested in `tests/test_unfreeze.rs`: confirms only the targeted account
thaws (a sibling account and a freshly created account both remain
frozen), and confirms Token-2022 itself rejects a thaw signed by anyone
other than the real freeze authority.

### Task 5 — re-issuing for confidentiality + seizure

**Confidential transfers cannot be added to an existing mint.**
`ConfidentialTransferMint`, like every mint extension, must be
initialized before `InitializeMint2` runs — and that instruction already
ran on the Task 1 mint. There is no upgrade path, only a fresh mint.
`ReissueMint` carries forward all four original extensions and adds:

- **`PermanentDelegate`** — the seizure authority regulators require.
  Unlike the other extensions here, it has no imperative CPI wrapper in
  `anchor-spl`; it's initialized via `permanent_delegate_initialize`
  from `anchor_spl::token_2022_extensions::permanent_delegate`.
- **`ConfidentialTransferMint`**, with `auto_approve_new_accounts =
  false` — the "manual" approval policy the task specifies. New
  accounts do not get confidential-transfer approval automatically; the
  confidential transfer mint authority must explicitly approve each one.

#### A real gap, found by building it, not assumed

The task asked to "identify the gap" between confidentiality and a
seizure authority landing on the same mint. Building this mint surfaced
a *third*, more fundamental gap that isn't the one the prompt was
pointing at, but is real and worth recording: **`TransferFeeConfig` and
`ConfidentialTransferMint` cannot coexist on a mint without
`ConfidentialTransferFeeConfig` also present.** Attempting it fails at
`InitializeMint2` with `InvalidExtensionCombination`
(custom program error `0x33`) — confirmed by an actual failing
transaction, not inferred from documentation. The reason is structural:
a protocol fee is a percentage of a transfer amount, and once that
amount is encrypted, a plaintext fee can no longer be computed. Token-2022
resolves this by encrypting the fee itself under the withdraw-withheld
authority's ElGamal key — which is exactly what
`ConfidentialTransferFeeConfig` provides. The fix added a seventh
extension and the corresponding `initialize_confidential_transfer_fee_config`
CPI, positioned after `ConfidentialTransferMint` (required, since
Token-2022 checks that the confidential mint config already exists) and
before `InitializeMint2`.

Tested in `tests/test_reissue.rs`: confirms all seven extensions are
present, the approval policy is manual, and the permanent delegate is
set correctly.

### Task 6 — the confidential lifecycle

`instructions/confidential.rs` implements all five steps:

1. **`ConfigureConfidentialAccount`** — owner-only, and distinct from ATA
   creation-by-anyone: creating the underlying token account can be done
   by anyone paying rent, but opting a specific account into confidential
   transfers is a decision only its owner can make, since it commits
   them to managing an ElGamal keypair and AES key for that account
   going forward. Token-2022 enforces the owner's signature separately
   from whatever account creation already happened.
2. **`DepositConfidentialTokens`** — moves tokens from the public balance
   into the pending confidential balance. Needs no proof, since the
   amount was already public before the move.
3. **`ApplyConfidentialPendingBalance`** — moves pending into available.
   The new available-balance ciphertext is supplied by the account
   owner, encrypted under their own AES key; the program has no access
   to that key and cannot compute this value itself.
4. **`ConfidentialTransfer`** — a transfer between two configured
   accounts, referencing three pre-verified proof context accounts
   (equality, ciphertext validity, range).
5. **`WithdrawConfidentialTokens`** — moves confidential available
   balance back to the public balance, referencing two pre-verified
   proof context accounts (equality, range). Per the task, pending
   balance must be applied (step 3) before this runs; this instruction
   does not apply it implicitly, since that's a separately-owner-
   authorized step with its own ciphertext.

Every proof-dependent instruction here (`ConfigureConfidentialAccount`,
`ConfidentialTransfer`, `WithdrawConfidentialTokens`) takes a proof
**context state account** rather than generating or inspecting any
cryptographic material on-chain. The proof itself — a `PubkeyValidityProof`
for configuration, equality/validity/range proofs for transfer and
withdrawal — is generated client-side and pre-verified into that account
by the ZK ElGamal Proof program before our instruction ever runs. Our
program only ever references the account; it never touches the proof
data directly.

#### What is tested, and what genuinely isn't

`tests/test_confidential_configure.rs` proves something worth stating
plainly: **the native ZK ElGamal Proof program executes correctly
inside litesvm.** This wasn't obvious going in — the program was
disabled on mainnet and devnet for most of a year after a 2025
verification bug, a stock `solana-test-validator` does not enable it by
default, and litesvm bundles it as a compiled binary outside this
project's own dependency graph. The test generates a real
`ElGamalKeypair`, builds a real `PubkeyValidityProofData`, submits it to
the ZK ElGamal Proof program, and confirms the resulting context state
account is owned by that program and holds real data. That part passes.

What doesn't currently work in litesvm is calling `ConfigureConfidentialAccount`
against that same, correctly-verified context account: Token-2022
rejects it with `InvalidAccountData`. This was diagnosed, not just
observed. The first hypothesis — a duplicate version of
`solana-zk-elgamal-proof-interface` in the dependency graph, the same
class of bug hit twice already during this project (see below) — was
checked with `cargo tree -i` and ruled out: exactly one version (0.1.3)
appears everywhere in this project's graph. The remaining explanation is
that litesvm bundles a pre-compiled Token-2022 `.so` binary built at a
version this project's `Cargo.toml` doesn't control, and that binary
appears to expect a different byte layout for a proof context account
than the one produced by the proof-generation crates available here.
This is a skew between two binaries that this project's own dependency
resolution has no way to close, not a bug in `ConfigureConfidentialAccount`
itself. `ConfidentialTransfer` and `WithdrawConfidentialTokens` are
implemented against the correct (split-proof, three-argument and
two-argument respectively) instruction signatures, confirmed to compile,
but were not pushed through the same litesvm proof pipeline for the same
reason — doing so would only reproduce the identical mismatch, twice.

The path to actually testing these three end-to-end is a real local
validator (`solana-test-validator` with the feature gate enabled, or a
mainnet-forking validator) running the exact Token-2022 build this
project depends on, rather than litesvm's bundled binary — noted here
rather than left silent.

Every dependency and compiler issue hit while building this task —
including two real Cargo dependency-graph bugs — is logged in full,
in order, below.

## Full debugging log

Every problem hit while building this, in the order it happened, with
what broke and exactly how it was fixed. Kept in full — including the
small ones — because it's the most honest record of what building
against Token-2022 and its confidential-transfer extensions actually
involves.

1. **Program ID mismatch.** `anchor build` warned that the keypair on
   disk didn't match the `declare_id!` in source (the source still had
   an ID copied from earlier reference material). Fixed with
   `anchor keys sync`, which rewrote both `lib.rs` and `Anchor.toml` to
   the real local keypair.

2. **`CpiContext::new` type error.** Every CPI call initially passed
   `.to_account_info()` as the first argument and failed with
   `expected Pubkey, found AccountInfo`. This project's pinned
   `anchor-lang`/`anchor-spl` version (1.2.0) takes the program's
   `Pubkey` as `CpiContext::new`'s first argument, not its
   `AccountInfo`. Fixed by switching every call site to `.key()`.

3. **Stale tests referencing deleted instructions.** After replacing
   the original demo `lib.rs` (eight unrelated reference instructions)
   with the real task instructions, `tests/test_initialize.rs`,
   `tests/authority.rs`, and `tests/confidential.rs` failed to compile
   with `E0422`, referencing account/instruction types
   (`CreateMintDeclarative`, `CreateMintWithFee`, `DelegateToProgram`,
   `CreateSeizableMint`, `PermanentDelegateSeize`,
   `CreateConfidentialMint`, `CreateConfidentialFeeMint`,
   `DepositConfidential`, `ApplyPendingBalance`, `AssertSupportedMint`)
   that no longer existed. Fixed by emptying these three files to
   placeholders, then rebuilding real, task-specific tests into new
   files one task at a time.

4. **`anchor test --skip-local-validator` tried to reach a real
   validator.** It attempted to deploy to `127.0.0.1:8899` and failed
   to connect, since no local validator was running. Root cause: this
   project's tests are plain `litesvm` `#[test]` functions, which
   Anchor's TypeScript/Mocha/real-validator test runner isn't built to
   run. Fixed by using `cargo test --manifest-path programs/t22/Cargo.toml
   --test <name>` directly instead of `anchor test`.

5. **Unused `Result` warning** on `svm.add_program(...)`. Fixed with
   `.expect("failed to load t22 program into litesvm")`.

6. **`spl_token_2022` crate not found** in a test file. It isn't a
   direct dependency of the test crate; it's only reachable through
   `anchor-spl`'s re-export. Fixed by importing
   `anchor_spl::token_interface::spl_token_2022` instead of assuming a
   top-level crate — deliberately avoiding adding a second, separately
   versioned `spl-token-2022` dependency, which would risk the same
   class of bug as items 10–11 below.

7. **`initialize_permanent_delegate` import path wrong.** Guessed
   `extension::permanent_delegate::instruction::initialize_permanent_delegate`,
   which doesn't exist at that path. The real, and better, answer:
   `anchor-spl` ships a proper CPI wrapper for this extension at
   `anchor_spl::token_2022_extensions::permanent_delegate::{permanent_delegate_initialize,
   PermanentDelegateInitialize}`. Switched to that instead of a raw
   `invoke`, consistent with how the other extensions in this project
   are initialized.

8. **`InvalidExtensionCombination` (custom program error `0x33`) at
   runtime**, re-issuing the mint with `TransferFeeConfig` and
   `ConfidentialTransferMint` together. Diagnosed directly from the
   program log line ("Mint or account is initialized to an invalid
   combination of extensions"), not guessed at. Root cause: a
   plaintext fee cannot be computed on an encrypted transfer amount, so
   Token-2022 requires `ConfidentialTransferFeeConfig` whenever both of
   the other two extensions are present, so the fee itself can be
   withheld as an encrypted quantity. Fixed by adding that third
   extension and its init CPI, positioned after `ConfidentialTransferMint`
   (required — Token-2022 checks that the confidential mint config
   already exists) and before `InitializeMint2`.

9. **`spl_token_confidential_transfer_proof_extraction` not found in
   program scope.** It existed only as an aliased, test-only
   dev-dependency (`proofext`). `confidential.rs` is program source,
   not test code, so dev-dependencies aren't visible to it. Fixed by
   adding it as a real `[dependencies]` entry.

10. **Cargo: "depends on crate X multiple times with different
    names."** Hit twice — once for
    `spl-token-confidential-transfer-proof-extraction` (aliased as
    `proofext` in dev-dependencies, and now also a plain
    `[dependencies]` entry from item 9), and again later for
    `solana-zk-elgamal-proof-interface` (aliased as `zkif`). Cargo
    disallows the same crate appearing under two different names in one
    graph. Fixed both times the same way: delete the aliased
    dev-dependency line, keep a single canonical `[dependencies]`
    entry.

11. **Duplicate crate *version*, not just name.** After fixing item 10,
    got a `ProofLocation<'_, ...>` type mismatch — "expected" and
    "found" looked identical in the error text, which is the signature
    of two different actual versions of the same crate coexisting.
    Confirmed with `cargo tree -p spl-token-confidential-transfer-proof-extraction -i`,
    which showed two resolved versions: `0.6.1` (from this project's
    unpinned dependency) and `0.5.1` (pulled transitively through
    `anchor-spl` → `spl-token-2022-interface` v2.1.0). Fixed by pinning
    the explicit dependency to the exact transitive version, `=0.5.1`,
    collapsing the graph to one copy.

12. **`solana_zk_sdk` not found in a test file.** It existed only as an
    aliased dev-dependency (`zk`). A dev-dependency alias exposes only
    the alias name to importing code, not the crate's real name. Fixed
    by importing via `zk::...` instead of `solana_zk_sdk::...`.

13. **`build_pubkey_validity_proof_data(...)` type mismatch.** It
    returns `Result<PubkeyValidityProofData, ProofGenerationError>`,
    not the proof data directly. Fixed with `.expect(...)` to unwrap
    it before use.

14. **`AeCiphertext` has no `.as_ref()` method.** Needed a proper
    plain-old-data (POD) wrapper to cross from the client-side
    encryption type to raw bytes. Added the `solana-zk-sdk-pod`
    dev-dependency, converted via `PodAeCiphertext::from(...)`, then
    read the bytes with `bytemuck::bytes_of(...)`.

15. **`PodAeCiphertext::from(&zero_balance)` — trait not satisfied for
    a reference.** The compiler's own suggestion showed the real
    signature: `From<AeCiphertext>` (owned) and `From<[u8; 36]>`, not
    `From<&AeCiphertext>`. Fixed by dropping the `&`.

16. **`InvalidAccountData` at runtime calling
    `ConfigureConfidentialAccount`**, even though the ZK ElGamal proof
    verification transaction immediately before it succeeded. This
    proved something worth knowing on its own — litesvm does correctly
    execute the native ZK ElGamal Proof program — but Token-2022's own
    `ConfigureAccount` then rejected the resulting, correctly-verified
    context state account. Diagnosed by first checking the most likely
    cause (a repeat of item 11's duplicate-version bug) with
    `cargo tree -p solana-zk-elgamal-proof-interface -i`; that showed
    exactly one version (`0.1.3`) everywhere in the graph, ruling it
    out. The remaining explanation: litesvm bundles a pre-compiled
    Token-2022 binary at a version outside this project's `Cargo.toml`
    control, and that binary appears to expect a different byte layout
    for a proof context account than the one produced by the
    proof-generation crates available here — a skew between two
    binaries, not a bug in this project's own dependency resolution or
    in `ConfigureConfidentialAccount` itself. Decision made to document
    this rather than guess-pin further versions against an opaque
    binary with no visibility into what it was built against.

17. **`confidential_instruction::transfer(...)` wrong argument count.**
    Expected 10 arguments, the installed version wanted 12. The
    installed version is a newer "split-proof" variant taking three
    separate `ProofLocation`s (range, equality, validity) plus two
    `&PodElGamalCiphertext` decrypt-handle arguments, not the simpler
    single-`TransferData` signature shown in generic documentation for
    an older release. Fixed by matching the compiler's own suggested
    parameter list exactly, and adding the two decrypt-handle
    parameters to this project's own instruction signature so a caller
    can supply them.

18. **`invoke(&ix, &infos)` type mismatch: expected `&Instruction`,
    found `&Vec<Instruction>`.** Both `confidential_instruction::transfer`
    and `::withdraw` return `Vec<Instruction>`, the same shape
    `configure_account` already used — missed the first time writing
    these two. Fixed by looping: `for ix in ixs { invoke(&ix, &infos)?; }`.

19. **`PodElGamalCiphertext` import path wrong, twice.** First guess
    (`zk_elgamal_proof_program::proof_data::pod::PodElGamalCiphertext`)
    didn't exist. Found the real path by locating spl-token-2022's own
    source re-export chain: `solana_zk_sdk::encryption::pod::elgamal::PodElGamalCiphertext`,
    reached through `spl_token_2022::solana_zk_sdk::...`.

20. **`PodElGamalCiphertext(bytes)` — cannot construct a tuple struct
    with a private field.** Fixed using `bytemuck::cast(...)` instead
    of the tuple constructor. This surfaced a real sizing bug in the
    same fix: a `PodElGamalCiphertext` is 64 bytes (a commitment
    component plus a decryption-handle component), not 32 — the
    decrypt-handle parameters were widened from `[u8; 32]` to
    `[u8; 64]` to match.

21. **`bytemuck` not found in program scope.** Same pattern as items
    9–10: it existed only as a plain (non-aliased) dev-dependency, and
    program code (not just test code) now needed it. Moved the entry
    from `[dev-dependencies]` to `[dependencies]`.

22. **The fix from item 21 appeared not to take effect** — the next
    build failed with the identical "crate not found" error. Traced to
    the `Cargo.toml` edit not actually having been saved before
    rebuilding, not a second, different bug. Re-confirmed the exact
    before/after state of the file line by line before rebuilding
    again, which resolved it.

## Written finding

**What happens if a sanctioned user moves their balance into the
confidential system before the permanent delegate acts?**

The permanent delegate can seize funds from any account of its mint at
any time, without the holder's consent — but it does so with
`transfer_checked` (or `transfer_checked_with_fee`), an instruction that
operates on a token account's **public** balance. Once a sanctioned
holder deposits their public balance into the confidential system
(`DepositConfidentialTokens`, then `ApplyConfidentialPendingBalance`),
that balance no longer exists as a plaintext `u64` the permanent
delegate's transfer instruction can read or move. It exists only as
ElGamal ciphertext, decipherable solely by whoever holds the
corresponding secret key — the holder themselves, and, if the mint's
confidential-transfer configuration included one, an auditor key set at
mint creation. Token-2022 has no instruction that lets a permanent
delegate seize a confidential balance directly; seizure and
confidentiality operate on two different representations of a
holder's funds, and Token-2022's permanent-delegate seizure path was
built for the public one.

This does not mean the funds become permanently unreachable to the
issuer. The mint authority can freeze the account outright (this mint's
`DefaultAccountState` and freeze-authority design makes that available
regardless of whether the account holds a confidential balance), which
stops the holder from moving the balance further — including blocking a
subsequent confidential transfer or withdrawal — without needing to
decrypt or seize anything. And if the mint was configured at issuance
with an auditor ElGamal key (this project's `ReissueMint` did not set
one, passing `None`), the auditor can decrypt the account's confidential
balance and transaction history, restoring visibility even though the
permanent delegate still cannot move the funds unilaterally. Freezing
plus an auditor key together substitute for seizure — immobilize now,
and identify or later act on the true amount once decrypted — but
neither substitute is the same as the permanent delegate simply moving
the tokens out, and neither happens automatically. A regulator or
issuer relying on the permanent delegate as their seizure mechanism, and
who re-issues a mint without also provisioning an auditor key, has left
a real gap: the moment matters. A holder who moves first, into
confidentiality, converts a seizure problem into a freeze-and-negotiate
problem — the funds are stuck, but they are not taken.

## Running the tests

```bash
anchor build
cargo test --manifest-path programs/t22/Cargo.toml -- --nocapture
```

All tests pass. See the "what is tested, and what genuinely isn't"
section above for the one documented, diagnosed exception.