/** @type {import('next').NextConfig} */
// Static export → ./out (plain HTML + assets) so the design-QA gate can render it via a local
// static server. No server runtime needed for the prototype.
const nextConfig = {
  output: 'export',
  images: { unoptimized: true },
  reactStrictMode: true,
};
export default nextConfig;
