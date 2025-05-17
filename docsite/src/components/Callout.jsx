import clsx from 'clsx'

import { Icon } from '@/components/Icon'

const styles = {
  note: {
    container:
      'bg-blue-500/5 ring-blue-500/20',
    title: 'text-blue-500',
    body: '[--tw-prose-background:theme(colors.blue.50)] prose-a:text-blue-700 text-blue-800 prose-code:text-blue-600',
  },
  warning: {
    container:
      'bg-yellow-500/5 ring-yellow-500/20',
    title: 'text-purple-700 dark:text-yellow-500',
    body: 'text-yellow-800 [--tw-prose-underline:theme(colors.yellow.400)] [--tw-prose-background:theme(colors.yellow.50)] prose-a:text-purple-700 prose-code:text-purple-700 dark:text-yellow-300 dark:[--tw-prose-underline:theme(colors.yellow.700)] dark:prose-code:text-yellow-300',
  },
}

const icons = {
  note: (props) => <Icon icon="lightbulb" {...props} />,
  warning: (props) => <Icon icon="warning" color="amber" {...props} />,
}

export function Callout({ type = 'note', title, children }) {
  let IconComponent = icons[type]
  return (
    <div className={clsx('my-6 flex gap-2.5 rounded-2xl p-6 ring-1', styles[type].container)}>
      <IconComponent className="h-8 w-8 flex-none" />
      <div className="ml-4 flex-auto">
        <p className={clsx('m-0 font-display text-xl', styles[type].title)}>
          {title}
        </p>
        <div className={clsx('prose mt-2.5', styles[type].body)}>
          {children}
        </div>
      </div>
    </div>
  )
}
