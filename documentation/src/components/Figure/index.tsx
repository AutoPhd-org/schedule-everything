/**
 * Figure and FigureRow: illustrated images for docs pages and the homepage.
 * Resolves static paths with the site's baseUrl, lazy-loads, sets intrinsic
 * width/height (to avoid layout shift), and supports a dark-mode variant.
 * Both components are registered globally in src/theme/MDXComponents.tsx.
 */
import React, {type CSSProperties, type ReactNode} from 'react';
import clsx from 'clsx';
import useBaseUrl from '@docusaurus/useBaseUrl';
import ThemedImage from '@theme/ThemedImage';

// Intrinsic display sizes (CSS pixels) of the images in static/img.
// Screenshots are captured at 2x (phones at 3x), so these are half/third size.
const IMAGE_SIZES: Record<string, [number, number]> = {
  'img/hero.webp': [1600, 900],
  'img/modes.webp': [1600, 780],
  'img/social-card.jpg': [1200, 630],
  'img/diagrams/how-it-works.svg': [1200, 590],
  'img/diagrams/daily-loop.svg': [1200, 620],
  'img/diagrams/todays-schedule.svg': [1200, 560],
  'img/screens/overview-j.webp': [1440, 900],
  'img/screens/overview-j-dark.webp': [1440, 900],
  'img/screens/overview-p.webp': [1440, 900],
  'img/screens/overview-p-dark.webp': [1440, 900],
  'img/screens/schedule-agents.webp': [1440, 900],
  'img/screens/calendar.webp': [1126, 1234],
  'img/screens/activity-gallery.webp': [1440, 1180],
  'img/screens/lightbox-dark.webp': [1440, 1180],
  'img/screens/weekly.webp': [1190, 1742],
  'img/screens/weekly-dark.webp': [1190, 1742],
  'img/screens/mobile-overview.webp': [390, 844],
  'img/screens/mobile-overview-p-dark.webp': [390, 844],
  'img/screens/mobile-activity.webp': [390, 844],
  'img/rmd_add.gif': [960, 540],
  'img/rmd_view.gif': [960, 540],
};

function normalize(src: string): string {
  return src.replace(/^\//, '');
}

export function imageSize(src: string): {width?: number; height?: number} {
  const size = IMAGE_SIZES[normalize(src)];
  return size ? {width: size[0], height: size[1]} : {};
}

type Variant = 'screenshot' | 'diagram' | 'phone' | 'plain';

function inferVariant(src: string): Variant {
  if (src.includes('/diagrams/')) {
    return 'diagram';
  }
  if (src.includes('/mobile-')) {
    return 'phone';
  }
  return 'screenshot';
}

export type FigureProps = {
  /** Path under static/, for example "img/screens/calendar.webp". */
  src: string;
  /** Optional image shown instead of `src` when the site is in dark mode. */
  darkSrc?: string;
  alt: string;
  caption?: ReactNode;
  variant?: Variant;
  /** CSS max-width of the figure, e.g. 560 or "70%". */
  maxWidth?: number | string;
  /** Load immediately instead of lazily (use only above the fold). */
  eager?: boolean;
  className?: string;
};

export default function Figure({
  src,
  darkSrc,
  alt,
  caption,
  variant,
  maxWidth,
  eager = false,
  className,
}: FigureProps): ReactNode {
  const lightUrl = useBaseUrl(`/${normalize(src)}`);
  const darkUrl = useBaseUrl(`/${normalize(darkSrc ?? src)}`);
  const resolvedVariant = variant ?? inferVariant(src);
  const style: CSSProperties | undefined =
    maxWidth !== undefined ? {maxWidth} : undefined;
  const imgProps = {
    alt,
    ...imageSize(src),
    loading: eager ? ('eager' as const) : ('lazy' as const),
    decoding: 'async' as const,
  };

  return (
    <figure
      className={clsx('se-figure', `se-figure--${resolvedVariant}`, className)}
      style={style}>
      {darkSrc ? (
        <ThemedImage sources={{light: lightUrl, dark: darkUrl}} {...imgProps} />
      ) : (
        <img src={lightUrl} {...imgProps} />
      )}
      {caption ? <figcaption>{caption}</figcaption> : null}
    </figure>
  );
}

export function FigureRow({
  columns = 3,
  children,
}: {
  columns?: number;
  children: ReactNode;
}): ReactNode {
  return (
    <div
      className="se-figure-row"
      style={{'--se-columns': columns} as CSSProperties}>
      {children}
    </div>
  );
}
