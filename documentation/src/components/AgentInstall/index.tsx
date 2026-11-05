/**
 * AgentInstall: tabbed install commands for the schedule-everything plugin in
 * Claude Code, OpenAI Codex, and Google Antigravity. The tab choice is shared
 * across pages (groupId "agent"). Used on the homepage and in the docs.
 */
import React, {type ReactNode} from 'react';
import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';
import CodeBlock from '@theme/CodeBlock';
import Translate from '@docusaurus/Translate';

const REPO = 'AutoPhd-org/schedule-everything';

export default function AgentInstall({
  notes = true,
}: {
  /** Show the short note under each tab's commands. */
  notes?: boolean;
}): ReactNode {
  return (
    <Tabs groupId="agent" queryString="agent">
      <TabItem value="claude" label="Claude Code" default>
        <CodeBlock language="bash" title="Terminal">
          {`claude plugin marketplace add ${REPO}
claude plugin install schedule-everything@schedule-everything`}
        </CodeBlock>
        {notes ? (
          <>
            <p>
              <Translate id="agentInstall.claude.note">
                {'Inside a Claude Code session, use the slash commands instead:'}
              </Translate>
            </p>
            <CodeBlock language="text" title="Claude Code">
              {`/plugin marketplace add ${REPO}
/plugin install schedule-everything@schedule-everything`}
            </CodeBlock>
          </>
        ) : null}
      </TabItem>
      <TabItem value="codex" label="Codex">
        <CodeBlock language="bash" title="Terminal">
          {`codex plugin marketplace add ${REPO}
codex plugin add schedule-everything@schedule-everything`}
        </CodeBlock>
        {notes ? (
          <p>
            <Translate
              id="agentInstall.codex.note"
              values={{catalog: <code>.agents/plugins/marketplace.json</code>}}>
              {'Codex reads the plugin from the repository marketplace catalog, {catalog}.'}
            </Translate>
          </p>
        ) : null}
      </TabItem>
      <TabItem value="antigravity" label="Antigravity">
        <CodeBlock language="bash" title="Terminal">
          {`git clone https://github.com/${REPO}.git
agy plugin install ./schedule-everything/plugins/schedule-everything`}
        </CodeBlock>
        {notes ? (
          <p>
            <Translate id="agentInstall.antigravity.note">
              {'Antigravity installs the plugin from a local clone of the repository.'}
            </Translate>
          </p>
        ) : null}
      </TabItem>
    </Tabs>
  );
}
