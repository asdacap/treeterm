import { useStore } from 'zustand'
import type { Application, ApplicationRenderProps, WorkspaceStore } from '../../renderer/types'
import { AppAvailability } from '../../renderer/types'
import SshUploadPane from '../../renderer/components/SshUploadPane'

// eslint-disable-next-line @typescript-eslint/no-empty-object-type -- intentional marker interface for tab state
export interface SshUploadState {}

function SshUploadTab({ workspace }: { workspace: WorkspaceStore }) {
  const connectionId = useStore(workspace, s => s.connectionId)
  return <SshUploadPane connectionId={connectionId} />
}

export const sshUploadApplication: Application<SshUploadState> = {
  id: 'ssh-upload',
  name: 'Upload',
  icon: '⬆',
  createInitialState: () => ({}),
  onWorkspaceLoad: () => ({ close: () => {}, dispose: () => {} }),
  render: (props: ApplicationRenderProps) => <SshUploadTab workspace={props.workspace} />,
  canClose: true,
  showInNewTabMenu: true,
  displayStyle: 'flex',
  isDefault: false,
  availability: AppAvailability.RemoteOnly,
}
