import { basePath } from '@/lib/shared'

interface ShotProps {
  src: string
  alt: string
  width?: number
}

export function Shot({ src, alt, width = 300 }: ShotProps) {
  return (
    <figure className="my-6 flex flex-col items-center gap-3">
      <img
        src={`${basePath}/screenshots/${src}`}
        alt={alt}
        width={width}
        className="rounded-xl border border-fd-border shadow-xl"
        loading="lazy"
      />
      <figcaption className="text-sm text-fd-muted-foreground text-center">{alt}</figcaption>
    </figure>
  )
}

export function Shots({ children }: { children: React.ReactNode }) {
  return <div className="my-6 flex flex-row flex-wrap justify-center gap-6 [&_figure]:my-0">{children}</div>
}
