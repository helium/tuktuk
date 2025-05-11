## Troubleshooting

### Common Issues and Solutions

#### Account Not Initialized Error
```
Program log: AnchorError caused by account: task. Error Code: AccountNotInitialized. Error Number: 3012. Error Message: The program expected this account to be already initialized.
```
This error is normal and can be safely ignored. It occurs when multiple crank turners attempt to execute the same task simultaneously. When the second crank turner tries to run the task, it fails because the first one already completed and closed the task account.

#### Task Not Running
If your task isn't being executed:

1. First, locate your task ID using the task list command:
   ```bash
   tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name>
   ```

2. If the task simulation looks successful but isn't running, try running it manually:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
   ```

3. To see the failed transaction in Solana Explorer (even for failing transactions), use the `--skip-preflight` flag:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id> --skip-preflight
   ```

#### Cron Job Stopped Running
If your cron job has stopped executing, it may have been removed from the queue due to insufficient funding or other issues. See the [Cron Task Requeuing](#cron-task-requeuing) section for instructions on how to requeue your cron job.

#### Task Fixed But Not Running
If you had a failing task that you've fixed, but crank turners are no longer attempting to run it, this is because crank turners ignore tasks after running out of retries. To resolve this:

1. Run the task manually using the task run command:
   ```bash
   tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
   ```

2. If the task succeeds, it will be marked as complete and removed from the queue.

