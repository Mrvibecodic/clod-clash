import { createMDX } from 'fumadocs-mdx/next'

const withMDX = createMDX()

const basePath = process.env.SITE_BASE_PATH ?? ''

const config = {
  output: 'export',
  basePath,
  assetPrefix: basePath || undefined,
  trailingSlash: true,
  reactStrictMode: true,
  images: { unoptimized: true },
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
}

export default withMDX(config)
