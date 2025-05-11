## Cron Tasks

Sometimes, it's helpful to run a task on a specific schedule. You can do this by creating a cron job. A cron job will queue tasks onto a task queue at a specific time. The following example will queue a task every minute. Note that you will need to keep the cron funded so that it can, in turn, fund the task queue for each task it creates.

See an example of creating a cron job here [typescript-examples](https://github.com/helium/tuktuk/typescript-examples/src/cron-memo.ts).