## Remote Transactions

Sometimes transactions are complicated enough that you cannot compile it ahead of time. An example of this may be a transaction that uses cNFTs and requires a proof. In this case, you can run a remote server that returns the set of instructions. This server will need to sign the instructions so the program can trust that they are associated with the given task.

Tuktuk will `POST` to the remote URL with the following JSON body:

```json
{
  "task": "<task-pubkey>",
  "task_queue": "<task-queue-pubkey>",
  "task_queued_at": "<task-queued-at-timestamp>"
}
```

Your server will need to return the following JSON body:

```json
{
  "transaction": "<base64-encoded-transaction>",
  "remaining_accounts": "<base64-encoded-remaining-accounts>",
  "signature": "<base64-encoded-signature>"
}
```

You can see an example of this in the [remote-server-example](./solana-programs/packages/remote-example-server/src/index.ts).

You can queue such a task by using `remoteV0` instead of `compileV0` in the `QueueTaskV0` instruction.

```typescript
await program.methods.queueTaskV0({
  id: taskId,
  trigger: { now: {} },
  transaction: {
    remoteV0: {
      url: "http://localhost:3002/remote",
      signer: me,
    },
  },
});
```