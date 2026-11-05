/**
 * Registers site components for every Markdown/MDX page, so docs can use
 * <Figure>, <FigureRow>, and <AgentInstall> without importing them.
 */
import MDXComponents from '@theme-original/MDXComponents';
import Figure, {FigureRow} from '@site/src/components/Figure';
import AgentInstall from '@site/src/components/AgentInstall';

export default {
  ...MDXComponents,
  Figure,
  FigureRow,
  AgentInstall,
};
