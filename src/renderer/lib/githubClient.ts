/**
 * GitHub Client for Renderer Process
 *
 * Migrated from main/index.ts — GitHub PR info fetching now runs directly in
 * the renderer using ExecApi and fetch instead of going through IPC to main.
 */

import type { ExecApi, GitHubApi, GitHubPrInfoResult, SettingsApi } from '../types'

// --- Helpers ---

type ExecResult = { exitCode: number; stdout: string; stderr: string }

async function execCommand(
  exec: ExecApi,
  connectionId: string,
  cwd: string,
  command: string,
  args: string[],
): Promise<ExecResult> {
  const startResult = await exec.start(connectionId, cwd, command, args)
  if (!startResult.success) throw new Error(startResult.error)
  const { execId } = startResult

  return new Promise((resolve, reject) => {
    const stdout: string[] = []
    const stderr: string[] = []

    const unsub = exec.onEvent(execId, (event) => {
      if (event.type === 'stdout') {
        stdout.push(event.data)
      } else if (event.type === 'stderr') {
        stderr.push(event.data)
      } else if (event.type === 'exit') {
        unsub()
        resolve({ exitCode: event.exitCode, stdout: stdout.join(''), stderr: stderr.join('') })
      } else {
        unsub()
        reject(new Error(event.message))
      }
    })
  })
}

export function parseGitHubOwnerRepo(remoteUrl: string): { owner: string; repo: string } | null {
  // Handle SSH: git@github.com:owner/repo.git
  const sshMatch = remoteUrl.match(/github\.com[:/]([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (sshMatch?.[1] && sshMatch[2]) return { owner: sshMatch[1], repo: sshMatch[2] }
  // Handle HTTPS: https://github.com/owner/repo.git
  const httpsMatch = remoteUrl.match(/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?$/)
  if (httpsMatch?.[1] && httpsMatch[2]) return { owner: httpsMatch[1], repo: httpsMatch[2] }
  return null
}

export function createGitHubApi(
  exec: ExecApi,
  settingsApi: SettingsApi,
  connectionId: string,
): GitHubApi {
  return {
    getPrInfo: async (repoPath: string, head: string, base: string): Promise<GitHubPrInfoResult> => {
      try {
        // Get GitHub token
        const settings = await settingsApi.load()
        let token: string
        if (settings.github.autodetectViaGh) {
          const result = await execCommand(exec, connectionId, repoPath, 'gh', ['auth', 'token'])
          if (result.exitCode !== 0) {
            return { error: 'Failed to get token from gh CLI. Is gh installed and authenticated?' }
          }
          token = result.stdout.trim()
        } else {
          token = settings.github.pat || ''
          if (!token) return { error: 'No GitHub PAT configured. Set one in Settings > GitHub.' }
        }

        // Get remote URL and parse owner/repo
        const remoteResult = await execCommand(exec, connectionId, repoPath, 'git', ['remote', 'get-url', 'origin'])
        if (remoteResult.exitCode !== 0) {
          return { error: `Failed to get remote URL: ${remoteResult.stderr}` }
        }
        const remoteUrl = remoteResult.stdout.trim()
        const parsed = parseGitHubOwnerRepo(remoteUrl)
        if (!parsed) return { error: `Could not parse GitHub owner/repo from remote URL: ${remoteUrl}` }
        const { owner, repo } = parsed

        // Search for existing PR via REST
        const prResponse = await fetch(
          `https://api.github.com/repos/${owner}/${repo}/pulls?head=${owner}:${head}&base=${base}&state=open`,
          { headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json' } }
        )
        if (!prResponse.ok) {
          return { error: `GitHub API error: ${String(prResponse.status)} ${prResponse.statusText}` }
        }
        const prs = await prResponse.json() as Array<{ number: number; title: string }>

        if (prs.length === 0) {
          return { noPr: true as const, createUrl: `https://github.com/${owner}/${repo}/compare/${base}...${head}?expand=1` }
        }

        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length checked above
        const pr = prs[0]!
        const prUrl = `https://github.com/${owner}/${repo}/pull/${String(pr.number)}`

        // Fetch rich PR info via GraphQL
        const graphqlQuery = `query($owner: String!, $repo: String!, $prNumber: Int!) {
          repository(owner: $owner, name: $repo) {
            pullRequest(number: $prNumber) {
              state
              reviewThreads(first: 100) {
                nodes {
                  isResolved
                  comments(first: 1) {
                    nodes {
                      body
                      path
                      line
                      author { login }
                    }
                  }
                }
              }
              latestReviews(first: 20) {
                nodes {
                  author { login }
                  state
                }
              }
              commits(last: 1) {
                nodes {
                  commit {
                    statusCheckRollup {
                      contexts(first: 50) {
                        nodes {
                          ... on CheckRun {
                            __typename
                            name
                            status
                            conclusion
                          }
                          ... on StatusContext {
                            __typename
                            context
                            state
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }`

        try {
          const graphqlResponse = await fetch('https://api.github.com/graphql', {
            method: 'POST',
            headers: {
              Authorization: `Bearer ${token}`,
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({
              query: graphqlQuery,
              variables: { owner, repo, prNumber: pr.number }
            })
          })

          if (!graphqlResponse.ok) {
            // Graceful degradation — return basic PR info
            return { prInfo: { number: pr.number, url: prUrl, title: pr.title, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 } }
          }

          const graphqlData = await graphqlResponse.json() as {
            data?: {
              repository?: {
                pullRequest?: {
                  state?: string
                  reviewThreads?: {
                    nodes?: Array<{
                      isResolved: boolean
                      comments?: { nodes?: Array<{ body: string; path: string; line: number | null; author?: { login: string } }> }
                    }>
                  }
                  latestReviews?: {
                    nodes?: Array<{ author?: { login: string }; state: string }>
                  }
                  commits?: {
                    nodes?: Array<{
                      commit?: {
                        statusCheckRollup?: {
                          contexts?: {
                            nodes?: Array<{
                              __typename: string
                              name?: string
                              status?: string
                              conclusion?: string | null
                              context?: string
                              state?: string
                            }>
                          }
                        }
                      }
                    }>
                  }
                }
              }
            }
          }

          const prData = graphqlData.data?.repository?.pullRequest
          const prState = (prData?.state ?? 'OPEN') as 'OPEN' | 'CLOSED' | 'MERGED'

          // Parse review threads
          const threads = prData?.reviewThreads?.nodes ?? []
          const unresolvedThreads = threads
            .filter(t => !t.isResolved)
            .map(t => {
              const firstComment = t.comments?.nodes?.[0]
              return {
                isResolved: false,
                path: firstComment?.path ?? '',
                body: firstComment?.body ?? '',
                author: firstComment?.author?.login ?? '',
                line: firstComment?.line ?? null,
              }
            })

          // Parse reviews
          const reviews = (prData?.latestReviews?.nodes ?? []).map(r => ({
            author: r.author?.login ?? '',
            state: r.state as 'APPROVED' | 'CHANGES_REQUESTED' | 'COMMENTED' | 'PENDING' | 'DISMISSED',
          }))

          // Parse check runs
          const commitNode = prData?.commits?.nodes?.[0]?.commit
          const contexts = commitNode?.statusCheckRollup?.contexts?.nodes ?? []
          const checkRuns = contexts
            .filter(c => c.__typename === 'CheckRun')
            .map(c => ({
              name: c.name ?? '',
              status: (c.status ?? 'QUEUED') as 'COMPLETED' | 'IN_PROGRESS' | 'QUEUED' | 'WAITING' | 'PENDING' | 'REQUESTED',
              conclusion: (c.conclusion ?? null) as 'SUCCESS' | 'FAILURE' | 'NEUTRAL' | 'CANCELLED' | 'TIMED_OUT' | 'ACTION_REQUIRED' | 'SKIPPED' | null,
            }))

          return {
            prInfo: {
              number: pr.number,
              url: prUrl,
              title: pr.title,
              state: prState,
              reviews,
              checkRuns,
              unresolvedThreads,
              unresolvedCount: unresolvedThreads.length,
            }
          }
        } catch {
          // GraphQL failed — graceful degradation
          return { prInfo: { number: pr.number, url: prUrl, title: pr.title, state: 'OPEN' as const, reviews: [], checkRuns: [], unresolvedThreads: [], unresolvedCount: 0 } }
        }
      } catch (error) {
        return { error: error instanceof Error ? error.message : 'Unknown error' }
      }
    }
  }
}
