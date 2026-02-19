Tari L2 Developer Guide: Outline & Brief

What this is

A step-by-step guide that takes someone from zero to running a wallet, writing a template, publishing a contract, and
interacting with it through an application.
The whole thing. By the end, the reader has done all four and understands what they did.

Who it's for

Testnet developers who want to build on Tari L2, not just move funds around. They're technical enough to follow code
but they shouldn't need to understand Tari internals to get started.

The core principle

Every section has two layers:

1. The walkthrough - minimal friction, minimal explanation. Get them to a working result fast. Full copy-paste examples.
   No "exercise for the reader" gaps.
2. The "what you just did" breakdown - after they've seen it work, then explain the mechanics. This is where you go
   deeper on architecture, protocol details, what's happening under the hood. It's optional reading that
   rewards curiosity but doesn't block progress.

The guide must ship with

- A ready-to-use template (complete, tested, sitting in the repo or available to pull)
- An already-published contract on testnet they can poke at before publishing their own
- A working application that talks to that contract (runnable out of the box)
- Each of these exists both as "here, use this one" and as "here's how to build your own from scratch"

  ---
Section Outline

1. Get Your Wallet Running

Goal: Reader has a running wallet connected to testnet with funds.

Walkthrough:

- Install prerequisites (keep this list short)
- Set up and run the wallet
- Get testnet funds (faucet or however this works)
- Send a transaction to confirm everything's live

What you achieved:

- Brief explanation of what the wallet is actually doing
- Where it fits in the L2 architecture
- What "connected to testnet" means in terms of the network

Exit state: Wallet running, funds available, one confirmed tx.

  ---

2. Write a Template

Goal: Reader has written a template from scratch and understands what it describes.

Walkthrough:

- Start from the provided reference template (full source, annotated)
- Walk through the structure: what each part does in plain terms
- Have them modify it (something concrete - change a value, add a field)
- Build/compile the template locally

What you achieved:

- What a template actually is in Tari's model
- How templates relate to contracts (template = blueprint, contract = instance)
- The type system, state model, and any constraints they should know about
- How this compares to smart contracts on other chains (brief, not a sales pitch)

Exit state: A compiled template ready to publish.

  ---

3. Publish the Contract

Goal: Reader has published their template as a live contract on testnet.

Walkthrough:

- Point them at the existing published contract first ("here's one we made earlier, go look at it")
- Show the publish command/process step by step
- Publish their template from Section 2
- Verify it's live (query it, see it on an explorer, whatever the tooling supports)

What you achieved:

- What happens during publishing (the transaction, the registration, where it lives)
- Contract addressing - how to find and reference it
- State initialization and what the contract looks like on-chain now
- Fees and any resource considerations

Exit state: Their contract is published and queryable on testnet.

  ---

4. Build an Application That Interacts with the Contract

Goal: Reader has a working application that reads from and writes to their contract.

Walkthrough:

- Start with the provided reference application (runs against the pre-published contract)
- Run it as-is so they see it work before they change anything
- Retarget it to their contract from Section 3
- Walk through: connecting to the network, calling a method, reading state, submitting a transaction
- Add one more interaction (a second method call, a state read, something concrete)

What you achieved:

- The client library / SDK and what it gives you
- Transaction lifecycle: submit, process, finalize
- Reading vs. writing state (and the cost difference)
- Error handling patterns and what can go wrong
- Where to go from here: more complex interactions, event listening, building real apps

Exit state: A running application that talks to their contract on testnet.

  ---
Writing guidelines for the developer building this

Do:

- Test every single command and code block before publishing. If it doesn't work on a fresh setup, it doesn't ship.
- Use real output in examples. Don't fabricate terminal output or API responses.
- Keep the walkthrough sections ruthlessly short. If you're explaining why in the walkthrough, you're in the wrong
  section. Move it to "what you achieved."
- Version-pin everything. Dependencies, toolchain versions, testnet endpoints. "Latest" isn't a version.
- Include troubleshooting for the things that actually go wrong, not hypothetical edge cases.

Don't:

- Assume they've read other docs. Each section should link to prerequisites but not depend on tribal knowledge.
- Bury the commands in prose. Code blocks should be visually obvious and copy-pasteable.
- Mix platforms without flagging it. If a step differs on macOS vs Linux, say so inline.
- Skip error states. Show what a failed publish looks like. Show what a rejected transaction looks like. They'll hit
  these.

Success criteria

When this guide is done, a developer who has never touched Tari should be able to sit down, follow it start to finish in
one session, and walk away with:

1. A running wallet with testnet funds
2. A template they wrote themselves
3. A contract they published to testnet
4. An application they built that interacts with that contract

If any of those four things require them to leave the guide and go figure something out elsewhere, the guide isn't done.

  ---
That's the brief. The developer building this should treat the reference template, published contract, and sample
application as first-class deliverables, not afterthoughts. The guide is only as good as the examples that back it up.