Short answer: everyone learns about new members from the **Commit** that adds them—not from the Welcome.

Here’s the clean sequence you should implement (works for both “invite by address” and tokenized links):

1. **Propose Add**
   A current member (typically a manager) proposes `Add(new_leaf_node)`.

2. **Commit (the source of truth)**
   The same member (or any committer referencing that proposal) sends a **Commit** that:

   * References the `Add` proposal,
   * Updates the ratchet tree (includes a fresh path),
   * Advances the group to epoch **E+1**,
   * Authenticates the new roster via the confirmation MAC.

   ➜ Your Delivery Service (DS) must **serialize handshake messages** (Commits) per group and **fan them out** to all existing members.

3. **Welcome (only for the joiner)**
   In parallel with the Commit, the adder creates a **Welcome** that gives the newcomer what they need to derive the exact state of epoch **E+1**. Only the newcomer consumes the Welcome. Existing members never see it and don’t need it.

4. **State convergence**

   * **Existing members**: learn the new membership the moment they process the **Commit** for epoch E+1 (they update their tree, roster, and context).
   * **New member**: learns and matches that same state when they process the **Welcome** (and optionally the referenced GroupInfo).

5. **After join**
   The newcomer may send application messages only under epoch **E+1**. Peers that haven’t yet processed the Commit will reject those messages as “wrong epoch” until they catch up.

Practical guardrails & UX:

* **Ordering:** Treat Commits as strictly ordered. DS should not allow two concurrent Commits; if two arrive, one wins and the other is retried in the next epoch.
* **Delivery policy:** It’s fine if DS delivers the newcomer’s app messages immediately; clients that are still on epoch E will drop them, then accept once they process E+1.
* **Offline members:** When a member comes back, they fetch the backlog of **handshake messages** (Commits) from DS, apply them in order, and thereby learn all membership changes they missed. No separate “roster sync” API is needed.
* **Verification:** Clients must verify that the **committer was authorized** (e.g., role=manager in the previous epoch if your roles policy requires it). If not, they reject the epoch.
* **External commit variant:** If your policy supports self-join via **External Commit**, the same rule holds: everyone learns from that **Commit** (now authored by the joiner), and the DS fans it out like any other handshake message.
* **UI events:** Show “Alice added Bob” only **after** the Commit authenticates locally; do not trigger on Welcome delivery (you don’t see it anyway).
* **Resilience to withholding:** If the DS maliciously withholds the Commit from some members, those members won’t accept the newcomer’s traffic (epoch mismatch). Once they obtain the missing Commit (e.g., via DS backlog or peer-assisted catch-up if you implement it), they converge.

TL;DR: implement membership discovery as a **pure consequence of processing Commits**. Welcome is solely for the new member to recreate that committed state.
