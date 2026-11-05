/**
 * Homepage: gradient hero with the product collage, supported agents, how it
 * works, illustrated feature sections, the daily loop, per-agent install
 * tabs, and a closing call to action. Strings use translate() so the zh
 * locale can override them in i18n/zh/code.json.
 */
import React, {type ReactNode} from 'react';
import clsx from 'clsx';
import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Translate, {translate} from '@docusaurus/Translate';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import CodeBlock from '@theme/CodeBlock';
import AgentInstall from '@site/src/components/AgentInstall';
import {imageSize} from '@site/src/components/Figure';

import styles from './index.module.css';

const REPO_URL = 'https://github.com/AutoPhd-org/schedule-everything';
const INSTALL_COMMAND = `curl -fsSL \\
  https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash`;

function Img({
  src,
  alt,
  className,
  eager = false,
}: {
  src: string;
  alt: string;
  className?: string;
  eager?: boolean;
}): ReactNode {
  return (
    <img
      src={useBaseUrl(`/${src}`)}
      alt={alt}
      className={className}
      {...imageSize(src)}
      loading={eager ? 'eager' : 'lazy'}
      decoding="async"
      {...(eager ? {fetchPriority: 'high' as const} : {})}
    />
  );
}

function Hero(): ReactNode {
  return (
    <header className={styles.hero}>
      <div className={styles.heroBackdrop} aria-hidden="true" />
      <div className={clsx('container', styles.heroInner)}>
        <p className={styles.heroEyebrow}>
          <span className={styles.heroDot} aria-hidden="true" />
          <Translate id="homepage.hero.eyebrow">
            Local-first · plain-text TOML · open source
          </Translate>
        </p>
        <Heading as="h1" className={styles.heroTitle}>
          <Translate id="homepage.hero.title.line1">
            Your AI agent plans the week.
          </Translate>{' '}
          <span className={styles.heroTitleAccent}>
            <Translate id="homepage.hero.title.line2">
              Persistent reminders keep you on it.
            </Translate>
          </span>
        </Heading>
        <p className={styles.heroLead}>
          <Translate id="homepage.hero.lead" values={{rmd: <code>rmd</code>}}>
            {
              'Schedule Everything turns a weekly routine into alarms at every block, with tasks, deadlines, and habits in the {rmd} CLI and a local browser workspace. Build it with Claude Code, Codex, or Antigravity.'
            }
          </Translate>
        </p>
        <div className={styles.heroButtons}>
          <Link
            className={clsx('button button--lg', styles.buttonLight)}
            to="/docs/intro">
            <Translate id="homepage.hero.cta.start">Get started</Translate>
            <span aria-hidden="true"> →</span>
          </Link>
          <Link
            className={clsx('button button--lg', styles.buttonGlass)}
            href={REPO_URL}>
            <Translate id="homepage.hero.cta.github">View on GitHub</Translate>
          </Link>
        </div>
        <div className={styles.heroShot}>
          <Img
            src="img/hero.webp"
            eager
            alt={translate({
              id: 'homepage.hero.image.alt',
              message:
                'The Schedule Everything browser workspace: the J-mode overview in light mode, the weekly overview in dark mode, and P mode at phone size',
            })}
          />
        </div>
      </div>
    </header>
  );
}

function AgentStrip(): ReactNode {
  const agents = ['Claude Code', 'Codex', 'Antigravity'];
  return (
    <section
      className={styles.strip}
      aria-label={translate({
        id: 'homepage.strip.label',
        message: 'Supported AI coding agents',
      })}>
      <div className="container">
        <div className={styles.stripInner}>
          <span className={styles.stripLabel}>
            <Translate id="homepage.strip.worksWith">Works with</Translate>
          </span>
          <ul className={styles.stripAgents}>
            {agents.map((agent) => (
              <li key={agent} className={styles.agentPill}>
                <span className={styles.agentDot} aria-hidden="true" />
                {agent}
              </li>
            ))}
          </ul>
          <span className={styles.stripMeta}>
            <Translate id="homepage.strip.meta">
              rmd CLI · browser workspace · macOS and Linux
            </Translate>
          </span>
        </div>
      </div>
    </section>
  );
}

function SectionHeader({
  eyebrow,
  title,
  children,
}: {
  eyebrow: ReactNode;
  title: ReactNode;
  children?: ReactNode;
}): ReactNode {
  return (
    <div className={styles.sectionHeader}>
      <p className={styles.eyebrow}>{eyebrow}</p>
      <Heading as="h2" className={styles.sectionTitle}>
        {title}
      </Heading>
      {children ? <p className={styles.sectionLead}>{children}</p> : null}
    </div>
  );
}

function HowItWorks(): ReactNode {
  const steps = [
    {
      title: translate({id: 'homepage.how.step1.title', message: 'Ask in plain words'}),
      body: (
        <Translate
          id="homepage.how.step1.body"
          values={{build: <code>build-schedule</code>}}>
          {
            'Tell your agent about your work, energy, and habits. The {build} skill interviews you and confirms a summary before writing anything.'
          }
        </Translate>
      ),
    },
    {
      title: translate({id: 'homepage.how.step2.title', message: 'The plugin runs rmd'}),
      body: (
        <Translate
          id="homepage.how.step2.body"
          values={{
            check: <code>rmd check</code>,
            sync: <code>rmd sync</code>,
            update: <code>rmd update</code>,
          }}>
          {
            'Deterministic commands do the bookkeeping: {check} validates files, {sync} reads and saves today’s plan, and {update} reloads reminders.'
          }
        </Translate>
      ),
    },
    {
      title: translate({id: 'homepage.how.step3.title', message: 'Your files keep time'}),
      body: (
        <Translate id="homepage.how.step3.body">
          {
            'Plain TOML and JSON on your computer drive the background reminders and the browser workspace. Schedule Everything makes no AI calls itself.'
          }
        </Translate>
      ),
    },
  ];
  return (
    <section className={styles.section}>
      <div className="container">
        <SectionHeader
          eyebrow={<Translate id="homepage.how.eyebrow">How it works</Translate>}
          title={
            <Translate id="homepage.how.title">
              Talk to your agent. Get a routine that rings.
            </Translate>
          }>
          <Translate
            id="homepage.how.lead"
            values={{
              plugin: <code>schedule-everything</code>,
              build: <code>build-schedule</code>,
              plan: <code>plan-day</code>,
            }}>
            {
              'The {plugin} plugin gives Claude Code, Codex, and Antigravity two skills: {build} writes your weekly routine, and {plan} fills today’s focus blocks with real tasks.'
            }
          </Translate>
        </SectionHeader>
        <div className={styles.diagram}>
          <Img
            src="img/diagrams/how-it-works.svg"
            alt={translate({
              id: 'homepage.how.diagram.alt',
              message:
                'You ask your AI coding agent; the Schedule Everything plugin runs rmd commands; rmd validates and writes local files that drive reminders and the browser workspace',
            })}
          />
        </div>
        <ol className={styles.steps}>
          {steps.map((step, index) => (
            <li key={step.title} className={styles.step}>
              <span className={styles.stepNumber}>{index + 1}</span>
              <Heading as="h3" className={styles.stepTitle}>
                {step.title}
              </Heading>
              <p>{step.body}</p>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}

type FeatureProps = {
  eyebrow: ReactNode;
  title: ReactNode;
  children: ReactNode;
  points?: ReactNode[];
  media: ReactNode;
  reverse?: boolean;
  docLink?: {to: string; label: ReactNode};
};

function Feature({
  eyebrow,
  title,
  children,
  points,
  media,
  reverse = false,
  docLink,
}: FeatureProps): ReactNode {
  return (
    <div className={clsx(styles.feature, reverse && styles.featureReverse)}>
      <div className={styles.featureText}>
        <p className={styles.eyebrow}>{eyebrow}</p>
        <Heading as="h3" className={styles.featureTitle}>
          {title}
        </Heading>
        <div className={styles.featureBody}>{children}</div>
        {points ? (
          <ul className={styles.points}>
            {points.map((point, index) => (
              <li key={index}>{point}</li>
            ))}
          </ul>
        ) : null}
        {docLink ? (
          <Link className={styles.featureLink} to={docLink.to}>
            {docLink.label}
            <span aria-hidden="true"> →</span>
          </Link>
        ) : null}
      </div>
      <div className={styles.featureMedia}>{media}</div>
    </div>
  );
}

function Tour(): ReactNode {
  return (
    <section className={clsx(styles.section, styles.sectionTinted)}>
      <div className="container">
        <SectionHeader
          eyebrow={<Translate id="homepage.tour.eyebrow">The workspace</Translate>}
          title={
            <Translate id="homepage.tour.title">
              A vivid, local browser workspace.
            </Translate>
          }>
          <Translate id="homepage.tour.lead" values={{web: <code>rmd web</code>}}>
            {
              'Run {web} to open it. It reads the same files as the CLI, listens only on 127.0.0.1, and keeps working without an agent.'
            }
          </Translate>
        </SectionHeader>

        <div className={styles.wideFeature}>
          <div className={styles.wideFeatureText}>
            <p className={styles.eyebrow}>
              <Translate id="homepage.modes.eyebrow">J mode · P mode</Translate>
            </p>
            <Heading as="h3" className={styles.featureTitle}>
              <Translate id="homepage.modes.title">Two ways to run your day</Translate>
            </Heading>
            <p className={styles.featureBody}>
              <Translate id="homepage.modes.body">
                J mode is the structured day: status orbs for overdue,
                due-today, and high-importance tasks and habits, what is
                happening right now, the next deadlines, tasks by type, and
                today’s timeline. P mode is for flexible days: the overview
                keeps only the tasks-by-type board in a warmer palette, and
                alarms for selected events pause.
              </Translate>
            </p>
            <p className={styles.modeSwitch}>
              <code>rmd mode j</code>
              <code>rmd mode p</code>
              <span>
                <Translate id="homepage.modes.switch">
                  or flip the J / P switch in the header
                </Translate>
              </span>
            </p>
          </div>
          <div className={styles.frame}>
            <Img
              src="img/modes.webp"
              alt={translate({
                id: 'homepage.modes.image.alt',
                message:
                  'J mode shows status, right now, deadlines, tasks, and the timeline; P mode shows only the tasks-by-type board in a darker, warmer palette',
              })}
            />
          </div>
        </div>

        <Feature
          eyebrow={<Translate id="homepage.plan.eyebrow">Schedule</Translate>}
          title={
            <Translate id="homepage.plan.title">
              Today’s plan, right on the timeline
            </Translate>
          }
          points={[
            <Translate key="1" id="homepage.plan.point1">
              Planned blocks show their task; reminders announce it
            </Translate>,
            <Translate key="2" id="homepage.plan.point2">
              Clear today’s plan in one click; your weekly routine never changes
            </Translate>,
            <Translate key="3" id="homepage.plan.point3">
              Install commands for each agent, with copy buttons
            </Translate>,
          ]}
          docLink={{
            to: '/docs/ai-agents',
            label: <Translate id="homepage.plan.link">AI agent plugins</Translate>,
          }}
          media={
            <div className={styles.frame}>
              <Img
                src="img/screens/schedule-agents.webp"
                alt={translate({
                  id: 'homepage.plan.image.alt',
                  message:
                    'The Schedule page with today’s timeline, the today’s plan card, and tabbed plugin install commands for Claude Code, Codex, and Antigravity',
                })}
              />
            </div>
          }>
          <p>
            <Translate
              id="homepage.plan.body"
              values={{
                plan: <code>plan-day</code>,
                pomodoro: <code>pomodoro</code>,
                potato: <code>potato</code>,
              }}>
              {
                'Ask “Plan my focus blocks for today” and the {plan} skill matches your most important tasks to today’s {pomodoro} and {potato} blocks, confirms with you, and saves the plan.'
              }
            </Translate>
          </p>
        </Feature>

        <Feature
          reverse
          eyebrow={
            <Translate id="homepage.calendar.eyebrow">Routines &amp; deadlines</Translate>
          }
          title={<Translate id="homepage.calendar.title">Your year at a glance</Translate>}
          points={[
            <Translate key="1" id="homepage.calendar.point1">
              Click a day to add a deadline
            </Translate>,
            <Translate key="2" id="homepage.calendar.point2">
              Pick one of eight marker colors, or keep the type color
            </Translate>,
            <Translate key="3" id="homepage.calendar.point3">
              Drag a chip onto another day to reschedule
            </Translate>,
          ]}
          docLink={{
            to: '/docs/browser#year-calendar',
            label: <Translate id="homepage.calendar.link">About the year calendar</Translate>,
          }}
          media={
            <div className={clsx(styles.frame, styles.framePortrait)}>
              <Img
                src="img/screens/calendar.webp"
                alt={translate({
                  id: 'homepage.calendar.image.alt',
                  message:
                    'An interactive year calendar with colored deadline days, an open day card for November 21, and draggable upcoming deadline chips',
                })}
              />
            </div>
          }>
          <p>
            <Translate id="homepage.calendar.body">
              Every deadline sits on a calendar for the whole year, tinted with
              its marker color, next to a list of what is coming up. Today’s
              habits live on the same page.
            </Translate>
          </p>
        </Feature>

        <Feature
          eyebrow={<Translate id="homepage.activity.eyebrow">Activity</Translate>}
          title={<Translate id="homepage.activity.title">Little wins add up</Translate>}
          points={[
            <Translate key="1" id="homepage.activity.point1">
              Filter by outcome or type, search, and sort
            </Translate>,
            <Translate key="2" id="homepage.activity.point2">
              Pick a day in the rhythm grid to see what you finished
            </Translate>,
            <Translate key="3" id="homepage.activity.point3">
              Open a card for its story: added, finished, and how fast
            </Translate>,
          ]}
          docLink={{
            to: '/docs/browser#activity-gallery-and-weekly-overview',
            label: <Translate id="homepage.activity.link">About the gallery</Translate>,
          }}
          media={
            <div className={styles.stack}>
              <div className={clsx(styles.frame, styles.stackBack)}>
                <Img
                  src="img/screens/activity-gallery.webp"
                  alt={translate({
                    id: 'homepage.activity.image.alt',
                    message:
                      'The Activity page with the weekly overview card, a rhythm grid of completed tasks per day, filters, and a gallery of finished tasks with generated artwork',
                  })}
                />
              </div>
              <div className={clsx(styles.frame, styles.stackFront)}>
                <Img
                  src="img/screens/lightbox-dark.webp"
                  alt={translate({
                    id: 'homepage.activity.lightbox.alt',
                    message:
                      'A task’s detail card in dark mode, showing when it was added and finished and how its time compares with other tasks',
                  })}
                />
              </div>
            </div>
          }>
          <p>
            <Translate id="homepage.activity.body">
              Activity is a gallery of everything you finished, cancelled, or
              let go, each card with artwork in its type’s color. More
              important work gets bigger cards.
            </Translate>
          </p>
        </Feature>

        <Feature
          reverse
          eyebrow={<Translate id="homepage.weekly.eyebrow">Weekly overview</Translate>}
          title={<Translate id="homepage.weekly.title">Your week in color</Translate>}
          points={[
            <Translate key="1" id="homepage.weekly.point1">
              Tasks completed, with an eight-week trend
            </Translate>,
            <Translate key="2" id="homepage.weekly.point2">
              Habit consistency, check-ins, and your streak
            </Translate>,
            <Translate key="3" id="homepage.weekly.point3">
              Daily rhythm, effort by type, pace, and highlights
            </Translate>,
          ]}
          media={
            <div className={clsx(styles.frame, styles.frameCropped)}>
              <Img
                src="img/screens/weekly-dark.webp"
                alt={translate({
                  id: 'homepage.weekly.image.alt',
                  message:
                    'The weekly overview in dark mode: tasks completed with an eight-week trend, habit rings, streak, daily rhythm, effort by type, outcomes, habit check-ins, pace, and highlights',
                })}
              />
            </div>
          }>
          <p>
            <Translate id="homepage.weekly.body">
              Open your week from the Activity page. Each card compares it with
              last week and with your average of up to four earlier weeks,
              mostly as charts.
            </Translate>
          </p>
        </Feature>

        <Feature
          eyebrow={<Translate id="homepage.phone.eyebrow">Any screen</Translate>}
          title={<Translate id="homepage.phone.title">Down to phone size, too</Translate>}
          points={[
            <Translate key="1" id="homepage.phone.point1">
              Light, dark, or system theme, remembered per browser
            </Translate>,
            <Translate key="2" id="homepage.phone.point2">
              Motion respects your reduced-motion setting
            </Translate>,
          ]}
          media={
            <div className={styles.phones}>
              <Img
                src="img/screens/mobile-overview.webp"
                className={styles.phone}
                alt={translate({
                  id: 'homepage.phone.overview.alt',
                  message: 'The J-mode overview at phone size',
                })}
              />
              <Img
                src="img/screens/mobile-overview-p-dark.webp"
                className={clsx(styles.phone, styles.phoneCenter)}
                alt={translate({
                  id: 'homepage.phone.p.alt',
                  message: 'P mode in the dark theme at phone size',
                })}
              />
              <Img
                src="img/screens/mobile-activity.webp"
                className={styles.phone}
                alt={translate({
                  id: 'homepage.phone.activity.alt',
                  message: 'The Activity page at phone size',
                })}
              />
            </div>
          }>
          <p>
            <Translate id="homepage.phone.body">
              The layout adapts to any window, from a wide monitor to a
              phone-sized viewport: the sidebar becomes an icon rail and cards
              stack.
            </Translate>
          </p>
        </Feature>
      </div>
    </section>
  );
}

function DailyLoop(): ReactNode {
  return (
    <section className={styles.section}>
      <div className="container">
        <SectionHeader
          eyebrow={<Translate id="homepage.loop.eyebrow">Every day</Translate>}
          title={<Translate id="homepage.loop.title">Reminders that stick.</Translate>}>
          <Translate id="homepage.loop.lead">
            Alarms sound and a dialog stays up at the start and end of every
            block until you respond. Deadlines, habits, and urgent tasks get
            their own nudges.
          </Translate>
        </SectionHeader>
        <div className={styles.diagram}>
          <Img
            src="img/diagrams/daily-loop.svg"
            alt={translate({
              id: 'homepage.loop.diagram.alt',
              message:
                'A loop of five steps: build your routine once, add tasks, plan today, focus with reminders, and look back',
            })}
          />
        </div>
      </div>
    </section>
  );
}

function Install(): ReactNode {
  const prompts = [
    translate({id: 'homepage.install.prompt1', message: 'Build my weekly schedule'}),
    translate({id: 'homepage.install.prompt2', message: 'Plan my focus blocks for today'}),
    translate({id: 'homepage.install.prompt3', message: 'Move my workouts to the evenings'}),
  ];
  return (
    <section id="install" className={clsx(styles.section, styles.sectionTinted)}>
      <div className="container">
        <SectionHeader
          eyebrow={<Translate id="homepage.install.eyebrow">Get started</Translate>}
          title={
            <Translate id="homepage.install.title">Up and running in three steps</Translate>
          }
        />
        <ol className={styles.installSteps}>
          <li className={styles.installStep}>
            <span className={styles.stepNumber}>1</span>
            <div className={styles.installBody}>
              <Heading as="h3" className={styles.stepTitle}>
                <Translate id="homepage.install.step1">Install Schedule Everything</Translate>
              </Heading>
              <p>
                <Translate id="homepage.install.step1.body">
                  The installer sets up Python, your configuration, and the
                  reminder service (LaunchAgent on macOS, systemd on Linux).
                </Translate>
              </p>
              <CodeBlock language="bash">{INSTALL_COMMAND}</CodeBlock>
            </div>
          </li>
          <li className={styles.installStep}>
            <span className={styles.stepNumber}>2</span>
            <div className={styles.installBody}>
              <Heading as="h3" className={styles.stepTitle}>
                <Translate id="homepage.install.step2">Add the plugin to your agent</Translate>
              </Heading>
              <AgentInstall />
            </div>
          </li>
          <li className={styles.installStep}>
            <span className={styles.stepNumber}>3</span>
            <div className={styles.installBody}>
              <Heading as="h3" className={styles.stepTitle}>
                <Translate id="homepage.install.step3">Ask your agent</Translate>
              </Heading>
              <ul className={styles.prompts}>
                {prompts.map((prompt) => (
                  <li key={prompt} className={styles.prompt}>
                    “{prompt}”
                  </li>
                ))}
              </ul>
              <p className={styles.installNote}>
                <Translate
                  id="homepage.install.step3.body"
                  values={{web: <code>rmd web</code>}}>
                  {
                    'Then run {web} to open the workspace. Your agent sends what it reads to its own model provider; your files stay on your computer.'
                  }
                </Translate>
              </p>
            </div>
          </li>
        </ol>
      </div>
    </section>
  );
}

function ClosingCta(): ReactNode {
  return (
    <section className={styles.section}>
      <div className="container">
        <div className={styles.cta}>
          <img
            src={useBaseUrl('/img/logo.svg')}
            alt=""
            width={64}
            height={64}
            className={styles.ctaLogo}
            loading="lazy"
          />
          <Heading as="h2" className={styles.ctaTitle}>
            <Translate id="homepage.cta.title">One day, one thing at a time.</Translate>
          </Heading>
          <p className={styles.ctaLead}>
            <Translate id="homepage.cta.lead">
              Free, open source, and local-first. Your routine is plain text you
              own.
            </Translate>
          </p>
          <div className={styles.heroButtons}>
            <Link
              className={clsx('button button--lg', styles.buttonLight)}
              to="/docs/quick-start">
              <Translate id="homepage.cta.start">Read the quick start</Translate>
            </Link>
            <Link
              className={clsx('button button--lg', styles.buttonGlass)}
              href={REPO_URL}>
              <Translate id="homepage.cta.github">Star on GitHub</Translate>
            </Link>
          </div>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title={translate({
        id: 'homepage.meta.title',
        message: 'Your AI agent plans the week',
      })}
      description={translate({
        id: 'homepage.meta.description',
        message:
          'A local-first weekly routine, task list, and focus-block reminder app with a browser workspace and the rmd CLI. Build your schedule with Claude Code, Codex, or Antigravity.',
      })}>
      <Hero />
      <main className={styles.main}>
        <AgentStrip />
        <HowItWorks />
        <Tour />
        <DailyLoop />
        <Install />
        <ClosingCta />
      </main>
    </Layout>
  );
}
