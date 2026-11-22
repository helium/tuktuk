## Create a task queue

First, you'll need to get some SOL to fund the task queue. You can get SOL from [Jupiter Aggregator](https://www.jup.ag/swap/USDC-hntyVP6YFm1Hg25TN9WGLqM12b8TQmcknKrdu1oxWux).

Next, create a task queue. A task queue has a default crank reward that will be used for all tasks in the queue, but each task can override this reward. Since crankers pay sol (and possibly priority fees) for each crank, the crank reward should be higher than the cost of a crank or crankers will not be incentivized to run your task.

Note that the `funding-amount` you specify is not inclusive of the 1 SOL minimum deposit. The funding amount will be used to pay the fees for tasks queued recursively (ie, by other tasks). 

```bash
tuktuk -u <your-solana-url> task-queue create --name <your-queue-name> --capacity 10 --funding-amount 100000000 --queue-authority <the-authority-to-queue-tasks> --min-crank-reward 1000000 --stale-task-age 172800
```

The queue capacity is the maximum number of tasks that can be queued at once. Higher capacity means more tasks can be queued, but it also costs more rent in SOL.