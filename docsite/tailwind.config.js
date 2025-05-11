const defaultTheme = require('tailwindcss/defaultTheme')

/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./src/**/*.{js,jsx}'],
  darkMode: 'class',
  theme: {
    container: {
      center: true,
      padding: "2rem",
      screens: {
        "2xl": "1400px",
      },
    },
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        // Custom colors based on the TukTuk logo
        yellow: {
          "50": "#fffde7",
          "100": "#fff9c4",
          "200": "#fff59d",
          "300": "#fff176",
          "400": "#ffee58",
          "500": "#ffeb3b",
          "600": "#fdd835",
          "700": "#fbc02d",
          "800": "#f9a825",
          "900": "#f57f17",
        },
        teal: {
          "50": "#e0f2f1",
          "100": "#b2dfdb",
          "200": "#80cbc4",
          "300": "#4db6ac",
          "400": "#26a69a",
          "500": "#009688",
          "600": "#00897b",
          "700": "#00796b",
          "800": "#00695c",
          "900": "#004d40",
        },
        red: {
          "50": "#ffebee",
          "100": "#ffcdd2",
          "200": "#ef9a9a",
          "300": "#e57373",
          "400": "#ef5350",
          "500": "#f44336",
          "600": "#e53935",
          "700": "#d32f2f",
          "800": "#c62828",
          "900": "#b71c1c",
        },
        purple: {
          "50": "#f3e5f5",
          "100": "#e1bee7",
          "200": "#ce93d8",
          "300": "#ba68c8",
          "400": "#ab47bc",
          "500": "#9c27b0",
          "600": "#8e24aa",
          "700": "#7b1fa2",
          "800": "#6a1b9a",
          "900": "#4a148c",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      keyframes: {
        "accordion-down": {
          from: { height: "0" },
          to: { height: "var(--radix-accordion-content-height)" },
        },
        "accordion-up": {
          from: { height: "var(--radix-accordion-content-height)" },
          to: { height: "0" },
        },
      },
      animation: {
        "accordion-down": "accordion-down 0.2s ease-out",
        "accordion-up": "accordion-up 0.2s ease-out",
      },
    },
  },
  plugins: [require('@tailwindcss/typography')],
}
