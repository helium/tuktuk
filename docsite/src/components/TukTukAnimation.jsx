"use client"

import { useEffect, useRef } from "react"
import Image from "next/image"

export default function TukTukAnimation() {
    const containerRef = useRef(null)
    const tuktukRef = useRef(null)
    const flameRef = useRef(null)

    useEffect(() => {
        const container = containerRef.current
        const tuktuk = tuktukRef.current
        const flame = flameRef.current

        if (!container || !tuktuk || !flame) return

        // Animation for the TukTuk vehicle
        const animateTukTuk = () => {
            // Slight bouncing animation
            let startTime = null
            const duration = 1000 // 1 second per bounce cycle

            const bounce = (timestamp) => {
                if (!startTime) startTime = timestamp
                const elapsed = timestamp - startTime
                const progress = (elapsed % duration) / duration

                // Simple sine wave for smooth bouncing
                const bounceHeight = Math.sin(progress * Math.PI * 2) * 5

                if (tuktuk) {
                    tuktuk.style.transform = `translateY(${bounceHeight}px)`
                }

                // Flame pulsing
                if (flame) {
                    const flameScale = 0.9 + Math.sin(progress * Math.PI * 2) * 0.1
                    flame.style.transform = `scale(${flameScale})`

                    // Randomize flame opacity slightly for flickering effect
                    const flickerOpacity = 0.85 + Math.random() * 0.15
                    flame.style.opacity = flickerOpacity.toString()
                }

                requestAnimationFrame(bounce)
            }

            requestAnimationFrame(bounce)
        }

        animateTukTuk()

        // Cleanup function
        return () => {
            // No cleanup needed for requestAnimationFrame in this case
            // as the component unmounting will stop the animation
        }
    }, [])

    return (
        <div ref={containerRef} className="relative w-full h-[300px] md:h-[400px]">
            {/* Background circle with gradient */}
            <div className="absolute inset-0 rounded-full bg-gradient-to-br from-[#5E5EEF]/60 via-[#7A7AF5]/40 to-[#4A4AD9]/30 opacity-50"></div>
            {/* TukTuk vehicle */}
            <div className="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2">
                <div ref={tuktukRef} className="relative transition-transform duration-300 ease-in-out">
                    <Image
                        src="/tuktuk-header-logo.png"
                        alt="TukTuk Logo"
                        width={200}
                        height={200}
                        className="w-auto h-auto max-w-[200px] md:max-w-[280px]"
                    />
                </div>
            </div>
        </div>
    )
}
