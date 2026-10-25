import large from '../assets/coo.webp';
import small from '../assets/coo-sm.webp';

/** Coo's portrait. Decorative: the name "Coo" is always shown or announced next to it. */
export function CooAvatar({ size = 28, className = '' }: { size?: number; className?: string }) {
  return (
    <img
      src={size > 48 ? large : small}
      width={size}
      height={size}
      alt=""
      className={`coo-avatar ${className}`.trim()}
      decoding="async"
      draggable={false}
    />
  );
}
