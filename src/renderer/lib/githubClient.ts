/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
/**
 * GitHub Client for Renderer Process
 *
 * Migrated from main/index.ts — GitHub PR info fetching now runs directly in
 * the renderer using ExecApi and fetch instead of going through IPC to main.
 */

import type { ExecApi, GitHubApi, GitHubPostCommentsResult, GitHubPrInfoResult, GitHubPrListResult, ReviewComment, SettingsApi } from '../types'
import { ExecEventType } from '../../shared/ipc-types'

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
      if (event.type === ExecEventType.Stdout) {
        stdout.push(event.data)
      } else if (event.type === ExecEventType.Stderr) {
        stderr.push(event.data)
      } else if (event.type === ExecEventType.Exit) {
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

type TokenAndRepo = { token: string; owner: string; repo: string }

async function resolveTokenAndRepo(
  exec: ExecApi,
  settingsApi: SettingsApi,
  connectionId: string,
  repoPath: string,
): Promise<TokenAndRepo | { error: string }> {
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
  return { token, owner: parsed.owner, repo: parsed.repo }
}

export function createGitHubApi(
  exec: ExecApi,
  settingsApi: SettingsApi,
  connectionId: string,
): GitHubApi {
  return {
    getPrInfo: async (repoPath: string, head: string, base: string): Promise<GitHubPrInfoResult> => {
      try {
        const resolved = await resolveTokenAndRepo(exec, settingsApi, connectionId, repoPath)
        if ('error' in resolved) return { error: resolved.error }
        const { token, owner, repo } = resolved

        // Search for existing PR via REST. Include closed/merged PRs so the
        // indicator can reflect a merged state — `state=open` would miss them
        // entirely and clear the indicator after merge.
        const prResponse = await fetch(
          `https://api.github.com/repos/${owner}/${repo}/pulls?head=${owner}:${head}&base=${base}&state=all`,
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
    },

    listOpenPrs: async (repoPath: string): Promise<GitHubPrListResult> => {
      try {
        const resolved = await resolveTokenAndRepo(exec, settingsApi, connectionId, repoPath)
        if ('error' in resolved) return { error: resolved.error }
        const { token, owner, repo } = resolved

        const response = await fetch(
          `https://api.github.com/repos/${owner}/${repo}/pulls?state=open&per_page=100&sort=updated&direction=desc`,
          { headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json' } }
        )
        if (!response.ok) {
          return { error: `GitHub API error: ${String(response.status)} ${response.statusText}` }
        }
        const prs = await response.json() as Array<{
          number: number
          title: string
          user: { login: string } | null
          head: { ref: string; repo: { full_name: string } | null }
        }>

        return {
          prs: prs.map(pr => ({
            number: pr.number,
            title: pr.title,
            author: pr.user?.login ?? '',
            headRefName: pr.head.ref,
            isCrossRepo: pr.head.repo?.full_name !== `${owner}/${repo}`,
          })),
        }
      } catch (error) {
        return { error: error instanceof Error ? error.message : 'Unknown error' }
      }
    },

    postReviewComments: async (
      repoPath: string,
      head: string,
      base: string,
      comments: ReviewComment[],
    ): Promise<GitHubPostCommentsResult> => {
      if (comments.length === 0) return { posted: 0, failed: [] }
      try {
        const resolved = await resolveTokenAndRepo(exec, settingsApi, connectionId, repoPath)
        if ('error' in resolved) return { error: resolved.error }
        const { token, owner, repo } = resolved

        // Find the open PR for this branch and its head commit SHA — inline
        // review comments must be anchored to a commit in the PR.
        const prResponse = await fetch(
          `https://api.github.com/repos/${owner}/${repo}/pulls?head=${owner}:${head}&base=${base}&state=all`,
          { headers: { Authorization: `Bearer ${token}`, Accept: 'application/vnd.github+json' } }
        )
        if (!prResponse.ok) {
          return { error: `GitHub API error: ${String(prResponse.status)} ${prResponse.statusText}` }
        }
        const prs = await prResponse.json() as Array<{ number: number; head: { sha: string } }>
        if (prs.length === 0) return { error: 'No open PR found for this branch' }
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- length checked above
        const pr = prs[0]!
        const commitId = pr.head.sha

        let posted = 0
        const failed: { id: string; error: string }[] = []
        for (const comment of comments) {
          const side = comment.side === 'modified' ? 'RIGHT' : 'LEFT'
          const res = await fetch(
            `https://api.github.com/repos/${owner}/${repo}/pulls/${String(pr.number)}/comments`,
            {
              method: 'POST',
              headers: {
                Authorization: `Bearer ${token}`,
                Accept: 'application/vnd.github+json',
                'Content-Type': 'application/json',
              },
              body: JSON.stringify({
                body: comment.text,
                commit_id: commitId,
                path: comment.filePath,
                line: comment.lineNumber,
                side,
              }),
            }
          )
          if (res.ok) {
            posted += 1
          } else {
            failed.push({ id: comment.id, error: `${String(res.status)} ${res.statusText}` })
          }
        }

        return { posted, failed }
      } catch (error) {
        return { error: error instanceof Error ? error.message : 'Unknown error' }
      }
    },
  }
}
