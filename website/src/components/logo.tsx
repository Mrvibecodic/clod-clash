import Image from 'next/image'
import icon from '../../public/icon.svg'

export function Logo({ className }: { className?: string }) {
  return <Image src={icon} alt="" className={className} priority />
}
