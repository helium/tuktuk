import { useState, useEffect } from 'react'
import Head from 'next/head'
import { useRouter } from 'next/router'
import clsx from 'clsx'
import { navigation } from '@/data/navigation'
import { ArrowRight, Clock, Zap, Shield, Code } from "lucide-react"
import { Button } from '@/components/ui/button'
import { MobileNavigation } from '@/components/MobileNavigation'
import TukTukAnimation from '@/components/TukTukAnimation'
import { HeroGradient } from '@/components/HeroGradient'
import Image from 'next/image'
import Link from 'next/link'
import { Search } from '@/components/Search'
import { CodeSnippet } from '@/components/CodeSnippet'
import { FeatureCard } from '@/components/FeaturedCard'

function GitHubIcon(props) {
    return (
        <svg aria-hidden="true" viewBox="0 0 16 16" {...props}>
            <path d="M8 0C3.58 0 0 3.58 0 8C0 11.54 2.29 14.53 5.47 15.59C5.87 15.66 6.02 15.42 6.02 15.21C6.02 15.02 6.01 14.39 6.01 13.72C4 14.09 3.48 13.23 3.32 12.78C3.23 12.55 2.84 11.84 2.5 11.65C2.22 11.5 1.82 11.13 2.49 11.12C3.12 11.11 3.57 11.7 3.72 11.94C4.44 13.15 5.59 12.81 6.05 12.6C6.12 12.08 6.33 11.73 6.56 11.53C4.78 11.33 2.92 10.64 2.92 7.58C2.92 6.71 3.23 5.99 3.74 5.43C3.66 5.23 3.38 4.41 3.82 3.31C3.82 3.31 4.49 3.1 6.02 4.13C6.66 3.95 7.34 3.86 8.02 3.86C8.7 3.86 9.38 3.95 10.02 4.13C11.55 3.09 12.22 3.31 12.22 3.31C12.66 4.41 12.38 5.23 12.3 5.43C12.81 5.99 13.12 6.7 13.12 7.58C13.12 10.65 11.25 11.33 9.47 11.53C9.76 11.78 10.01 12.26 10.01 13.01C10.01 14.08 10 14.94 10 15.21C10 15.42 10.15 15.67 10.55 15.59C13.71 14.53 16 11.53 16 8C16 3.58 12.42 0 8 0Z" />
        </svg>
    )
}

function Header({ navigation }) {
    let [isScrolled, setIsScrolled] = useState(false)

    useEffect(() => {
        function onScroll() {
            setIsScrolled(window.scrollY > 0)
        }
        onScroll()
        window.addEventListener('scroll', onScroll, { passive: true })
        return () => {
            window.removeEventListener('scroll', onScroll)
        }
    }, [])

    return (
        <header
            className={clsx(
                'sticky top-0 z-50 flex h-14 flex-wrap items-center justify-between px-4 transition duration-500 sm:px-6 lg:px-8 border-b border-gray-200',
                isScrolled
                    ? 'bg-white/[var(--bg-opacity-light)] backdrop-blur-md'
                    : 'bg-transparent'
            )}
        >
            <div className="mr-6 flex lg:hidden">
                <MobileNavigation navigation={navigation} />
            </div>
            <div className="relative flex flex-grow basis-0 items-center">
                <Link href="/" aria-label="Home page" className="flex gap-3">
                    <div className='flex items-center gap-2'>
                        <Image src="/tuktuk-logo.png" alt="TukTuk Logo" width={32} height={32} />
                        <span className='text-lg font-bold'>TukTuk</span>
                    </div>
                </Link>
                <nav className="hidden md:flex gap-6 ms-10">
                    <Link
                        href="#features"
                        className="flex items-center text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
                    >
                        Features
                    </Link>
                    <Link
                        href="#how-it-works"
                        className="flex items-center text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
                    >
                        How It Works
                    </Link>
                    {/* <Link
                        href="#pricing"
                        className="flex items-center text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
                    >
                        Pricing
                    </Link> */}
                    <Link
                        href="/docs"
                        className="flex items-center text-sm font-medium text-muted-foreground transition-colors hover:text-foreground"
                    >
                        Docs
                    </Link>
                </nav>
            </div>
            <div className="relative flex basis-0 items-center justify-end gap-4 md:flex-grow">
                <Search />
                <Link href="https://github.com/helium/tuktuk" className="group" aria-label="GitHub">
                    <GitHubIcon className="h-6 w-6 fill-zinc-400 group-hover:fill-zinc-500 dark:group-hover:fill-zinc-300" />
                </Link>
            </div>
        </header>
    )
}

export default function Home() {
    let router = useRouter()

    return (
        <>
            <Head>
                <title>TukTuk</title>
            </Head>
            <div className='overflow-hidden'>
                <Header navigation={navigation} />
                {/* ---- HERO --- */}
                <section className="relative overflow-hidden py-20 md:py-32">
                    <HeroGradient />
                    <div className="container relative z-10 px-4 md:px-6">
                        <div className="grid gap-6 lg:grid-cols-[1fr_400px] lg:gap-12 xl:grid-cols-[1fr_600px]">
                            <div className="flex flex-col justify-center space-y-4">
                                <div className="space-y-2">
                                    <div className=" gap-2 inline-flex items-center rounded-lg bg-[#E7DEFF] px-3 py-1 text-sm font-medium text-[#5E25FD] dark:bg-[#5E25FD] dark:text-white">
                                        <Image src="/helium-logo.svg" alt="Helium Logo" width={20} height={20} />
                                        Powering Helium
                                    </div>
                                    <h1 className="text-4xl font-bold tracking-tighter sm:text-5xl md:text-6xl">
                                        Schedule Tasks on Solana <span className="text-[#5E25FD]">Without the Wait</span>
                                    </h1>
                                    <p className="max-w-[600px] text-muted-foreground md:text-xl">
                                        TukTuk delivers reliable, blazing-fast cron jobs for your Solana applications. Set it, forget
                                        it, and watch your tasks execute with precision.
                                    </p>
                                </div>
                                <div className="flex flex-col gap-2 min-[400px]:flex-row">
                                    <Button className="bg-[#5E25FD] hover:bg-[#5E25FD] dark:bg-[#7F52FF] dark:hover:bg-[#7F52FF]" onClick={() => router.push('/docs')}>
                                        View Documentation
                                        <ArrowRight className="ml-2 h-4 w-4" />
                                    </Button>
                                </div>

                            </div>
                            <div className="flex items-center justify-center lg:justify-end">
                                <div className="w-full max-w-[400px] lg:max-w-none">
                                    <TukTukAnimation />
                                </div>
                            </div>
                        </div>
                    </div>
                </section>

                <section id="features" className="py-20 bg-slate-50 dark:bg-slate-900/50">
                    <div className="container px-4 md:px-6">
                        <div className="flex flex-col items-center justify-center space-y-4 text-center">
                            <div className="space-y-2">
                                <div className="inline-block rounded-lg bg-purple-100 px-3 py-1 text-sm font-medium text-purple-800 dark:bg-purple-900/30 dark:text-[#5E25FD]">
                                    Features
                                </div>
                                <h2 className="text-3xl font-bold tracking-tighter sm:text-4xl md:text-5xl">Why Choose TukTuk?</h2>
                                <p className="max-w-[900px] text-muted-foreground md:text-xl/relaxed lg:text-base/relaxed xl:text-xl/relaxed">
                                    Designed for developers who need reliable, fast, and secure on-chain automation.
                                </p>
                            </div>
                        </div>
                        <div className="mx-auto grid max-w-5xl items-center gap-6 py-12 md:grid-cols-2 lg:grid-cols-3 lg:gap-12">
                            <FeatureCard
                                icon={<Zap className="h-10 w-10 text-yellow-500" />}
                                title="Lightning Fast"
                                description="Execute tasks with minimal latency. TukTuk is optimized for speed at every step of the process."
                            />
                            <FeatureCard
                                icon={<Clock className="h-10 w-10 text-[#5E25FD]" />}
                                title="Precise Scheduling"
                                description="Set up complex schedules with cron syntax."
                            />
                            <FeatureCard
                                icon={<Shield className="h-10 w-10 text-red-500" />}
                                title="Bulletproof Reliability"
                                description="Built-in redundancy ensures your tasks run on time, every time, with automatic retries."
                            />
                            <FeatureCard
                                icon={<Code className="h-10 w-10 text-[#5E25FD]" />}
                                title="Developer Friendly"
                                description="Simple API, comprehensive SDKs, and detailed documentation make integration a breeze."
                            />
                            <FeatureCard
                                icon={
                                    <svg
                                        className="h-10 w-10 text-[#5E25FD]"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        xmlns="http://www.w3.org/2000/svg"
                                    >
                                        <path
                                            d="M21 12C21 16.9706 16.9706 21 12 21C7.02944 21 3 16.9706 3 12C3 7.02944 7.02944 3 12 3C16.9706 3 21 7.02944 21 12Z"
                                            stroke="currentColor"
                                            strokeWidth="2"
                                        />
                                        <path d="M12 7V12L15 15" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                                    </svg>
                                }
                                title="Security"
                                description="Smart contracts have not been audited yet and will be audited soon. Use at your own risk."
                            />

                            <FeatureCard
                                icon={
                                    <svg
                                        className="h-10 w-10 text-[#5E25FD]"
                                        viewBox="0 0 24 24"
                                        fill="none"
                                        xmlns="http://www.w3.org/2000/svg"
                                    >
                                        <path
                                            d="M13 2L3 14H12L11 22L21 10H12L13 2Z"
                                            stroke="currentColor"
                                            strokeWidth="2"
                                            strokeLinecap="round"
                                            strokeLinejoin="round"
                                        />
                                    </svg>
                                }
                                title="Cost-Effective"
                                description="Optimize your Solana operations with efficient task scheduling and execution patterns."
                            />
                        </div>
                    </div>
                </section>

                <section id="how-it-works" className="py-20">
                    <div className="container px-4 md:px-6">
                        <div className="flex flex-col items-center justify-center space-y-4 text-center">
                            <div className="space-y-2">
                                <div className="inline-block rounded-lg bg-purple-100 px-3 py-1 text-sm font-medium text-purple-800 dark:bg-purple-900/30 dark:text-[#5E25FD]">
                                    How It Works
                                </div>
                                <h2 className="text-3xl font-bold tracking-tighter sm:text-4xl md:text-5xl">
                                    Simple Integration, Powerful Results
                                </h2>
                                <p className="max-w-[900px] text-muted-foreground md:text-xl/relaxed lg:text-base/relaxed xl:text-xl/relaxed">
                                    Get up and running in minutes with our straightforward API.
                                </p>
                            </div>
                        </div>
                        <div className="mx-auto mt-12 max-w-4xl">
                            <CodeSnippet />
                        </div>
                        <div className="mt-16 grid gap-6 md:grid-cols-3">
                            <div className="flex flex-col items-center space-y-2 rounded-lg border p-6 text-center">
                                <div className="flex h-12 w-12 items-center justify-center rounded-full bg-purple-100 text-purple-900 dark:bg-purple-900/20 dark:text-purple-400">
                                    1
                                </div>
                                <h3 className="text-xl font-bold">Connect</h3>
                                <p className="text-sm text-muted-foreground">
                                    Integrate TukTuk with your dApp or smart contract using our SDK.
                                </p>
                            </div>
                            <div className="flex flex-col items-center space-y-2 rounded-lg border p-6 text-center">
                                <div className="flex h-12 w-12 items-center justify-center rounded-full bg-yellow-100 text-yellow-900 dark:bg-yellow-900/20 dark:text-yellow-400">
                                    2
                                </div>
                                <h3 className="text-xl font-bold">Schedule</h3>
                                <p className="text-sm text-muted-foreground">
                                    Define when and how often your tasks should run using cron syntax.
                                </p>
                            </div>
                            <div className="flex flex-col items-center space-y-2 rounded-lg border p-6 text-center">
                                <div className="flex h-12 w-12 items-center justify-center rounded-full bg-red-100 text-red-900 dark:bg-red-900/20 dark:text-red-400">
                                    3
                                </div>
                                <h3 className="text-xl font-bold">Relax</h3>
                                <p className="text-sm text-muted-foreground">
                                    TukTuk handles the rest, ensuring your tasks execute reliably and on time.
                                </p>
                            </div>
                        </div>
                    </div>
                </section>
            </div>
            <footer className="border-t bg-slate-50 dark:bg-slate-900/50">
                <div className="container flex flex-col gap-6 py-12 px-4 md:px-6 md:flex-row md:justify-between">
                    <div className="flex flex-col gap-6 md:max-w-sm">
                        <Link href="/" className="flex items-center space-x-2">
                            <Image src="/tuktuk-logo.png" alt="TukTuk Logo" width={40} height={40} className="h-10 w-auto" />
                            <span className="inline-block font-bold text-xl">TukTuk</span>
                        </Link>
                        <p className="text-sm text-muted-foreground">
                            Blazing fast on-chain cron jobs for the modern Solana developer. Schedule, automate, and relax.
                        </p>
                        <div className="flex gap-4">
                            <Link href="https://github.com/helium/tuktuk" className="text-muted-foreground hover:text-foreground" target="_blank">
                                <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
                                    <path
                                        fillRule="evenodd"
                                        d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z"
                                        clipRule="evenodd"
                                    ></path>
                                </svg>
                                <span className="sr-only">GitHub</span>
                            </Link>
                        </div>
                    </div>
                </div>
                <div className="border-t py-6">
                    <div className="container flex flex-col items-center justify-between gap-4 px-4 md:px-6 md:flex-row">
                        <p className="text-xs text-muted-foreground">
                            &copy; {new Date().getFullYear()} TukTuk. All rights reserved.
                        </p>
                        <p className="text-xs text-muted-foreground">Crafted with ❤️ for the Solana community</p>
                    </div>
                </div>
            </footer>
        </>
    )
}