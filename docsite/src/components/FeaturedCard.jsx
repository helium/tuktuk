
export function FeatureCard({ icon, title, description }) {
    return (
        <div className="flex flex-col items-center text-center p-6 space-y-4 rounded-lg border bg-background hover:shadow-md transition-shadow">
            <div className="p-2 rounded-full bg-slate-100 dark:bg-slate-800">{icon}</div>
            <h3 className="text-xl font-bold">{title}</h3>
            <p className="text-sm text-muted-foreground">{description}</p>
        </div>
    )
}
