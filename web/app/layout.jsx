import "./globals.css";

export const metadata = {
  title: "node-sec · analyst console",
  description: "Risk-prioritised fraud case review",
};

export default function RootLayout({ children }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body>{children}</body>
    </html>
  );
}
