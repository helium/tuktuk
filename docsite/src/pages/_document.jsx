import { Head, Html, Main, NextScript } from 'next/document'

export default function Document() {
  return (
    <Html className="antialiased [font-feature-settings:'ss01'] scroll-smooth" lang="en">
      <Head />
      <body className="bg-white overflow-x-hidden ">
        <Main />
        <NextScript />
      </body>
    </Html>
  )
}
