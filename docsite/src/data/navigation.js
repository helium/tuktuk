export const navigation = [
  {
    title: "Introduction",
    icon: (
      <svg
        width="18"
        height="21"
        viewBox="0 0 18 21"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <path
          fillRule="evenodd"
          clipRule="evenodd"
          d="M3.78058 7.87629C3.28841 8.25488 3 8.8406 3 9.46154V16.4767C3 17.5813 3.89543 18.4767 5 18.4767H14C15.1046 18.4767 16 17.5813 16 16.4767V9.46154C16 8.8406 15.7116 8.25488 15.2194 7.87629L10.7194 4.41475C10.0005 3.86175 8.99948 3.86175 8.28058 4.41475L3.78058 7.87629Z"
          fill="#5E25FD"
        />
      </svg>
    ),
    links: [
      { title: "Overview", href: "/docs/overview" },
      { title: "Running a Crank Turner", href: "/docs/running-a-crank-turner" },
    ],
  },
  {
    title: "Learn",
    icon: (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="18"
        height="21"
        fill="none"
      >
        <path
          fill="#5E25FD"
          d="M1 6.313a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1v-10ZM6 6.313a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1v-10Z"
        />
        <path
          fill="#B99BFE"
          d="M10.774 7.001a1 1 0 0 1 .707-1.225l1.932-.517a1 1 0 0 1 1.225.707l2.588 9.66a1 1 0 0 1-.707 1.224l-1.932.517a1 1 0 0 1-1.225-.707l-2.588-9.659Z"
        />
      </svg>
    ),
    links: [
      { title: "Prerequisites", href: "/docs/learn/prerequisites" },
      { title: "Installation", href: "/docs/learn/installation" },
      { title: "Quickstart", href: "/docs/learn/quickstart" },
      { title: "Create a Task Queue", href: "/docs/learn/create_a_task_queue" },
      {
        title: "Closing a Task Queue",
        href: "/docs/learn/closing_a_task_queue",
      },
      {
        title: "Funding a Task Queue",
        href: "/docs/learn/funding_a_task_queue",
      },
      {
        title: "Adding Queue Authorities",
        href: "/docs/learn/adding_queue_authorities",
      },
      { title: "Queue a Task", href: "/docs/learn/queue_a_task" },
      { title: "Remote Transactions", href: "/docs/learn/remote_transactions" },
      {
        title: "Monitoring the Task Queue",
        href: "/docs/learn/monitoring_the_task_queue",
      },
      { title: "Cron Tasks", href: "/docs/learn/cron_tasks" },
      {
        title: "Monitoring the Cron Job",
        href: "/docs/learn/monitoring_the_cron_job",
      },
      { title: "Running a Task", href: "/docs/learn/running_a_task" },
      { title: "Closing Tasks", href: "/docs/learn/closing_tasks" },
      { title: "Development Setup", href: "/docs/learn/development_setup" },
      { title: "Troubleshooting", href: "/docs/learn/troubleshooting" },
    ],
  },
  {
    title: "API",
    icon: (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width="18"
        height="21"
        fill="none"
      >
        <path
          fill="#5E25FD"
          d="M12.5 6A1.5 1.5 0 0 1 14 7.5v10a1.5 1.5 0 0 1-1.5 1.5H4a1 1 0 0 1-1-1V7.5A1.5 1.5 0 0 1 4.5 6h8Z"
          opacity=".4"
        />
        <path
          fill="#5E25FD"
          d="M16 14a2 2 0 0 1-2 2H6a1 1 0 0 1-1-1V5a2 2 0 0 1 2-2h7a2 2 0 0 1 2 2v9Z"
        />
        <path
          fill="#9f75fd"
          d="M13 6a1 1 0 1 1 0 2H8a1 1 0 0 1 0-2h5ZM13 9a1 1 0 1 1 0 2H8a1 1 0 0 1 0-2h5Z"
          opacity=".5"
        />
      </svg>
    ),
    links: [
      // DOCS NAVIGATION START
      { title: "Tuktuk", href: "/docs/api/tuktuk-sdk" },

      { title: "Cron", href: "/docs/api/cron-sdk" },

      { title: "Cpi-example", href: "/docs/api/cpi-example-sdk" },

      // DOCS NAVIGATION END
    ],
  },
]
