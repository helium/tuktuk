## Adding Queue Authorities

Task queues are meant to be reused for multiple use cases. As such, there can be multiple wallets that have the authority to queue tasks. Note that this authority should not be given out blindly, as the authority can queue tasks that use up task queue funding, and can use the task queue's custom signers.

You can add queue authorities to a task queue by using the `add-queue-authority` command. Queue authorities can queue tasks on behalf of other users.

```bash
tuktuk -u <your-solana-url> task-queue add-queue-authority --task-queue-name <your-queue-name> --queue-authority <the-authority-to-queue-tasks>
```

An example use case for multiple authorities at Helium is that we have a program, hpl-crons, that allows users to
create specific jobs that automate helium tasks relating to things like their staked positions. Because we have audited these specific tasks, we allow a PDA signer of the hpl-crons program to queue tasks on behalf of users. Simultaneously, we also have an admin authority that can queue or remove tasks for the sake of troubleshooting.