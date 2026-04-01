import React, { Component, ReactNode } from 'react'

export interface FallbackProps {
  error: Error
  reset: () => void
}

interface ErrorBoundaryProps<FP extends FallbackProps = FallbackProps> {
  children: ReactNode
  fallback?: ReactNode
  FallbackComponent?: React.ComponentType<FP>
  fallbackProps?: Omit<FP, keyof FallbackProps>
  onError?: (error: Error, errorInfo: React.ErrorInfo) => void
}

interface ErrorBoundaryState {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundary<FP extends FallbackProps = FallbackProps> extends Component<ErrorBoundaryProps<FP>, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps<FP>) {
    super(props)
    this.state = { hasError: false, error: null }
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo): void {
    console.error('ErrorBoundary caught an error:', error, errorInfo)

    if (this.props.onError) {
      this.props.onError(error, errorInfo)
    }
  }

  reset = (): void => {
    this.setState({ hasError: false, error: null })
  }

  render(): ReactNode {
    if (this.state.hasError && this.state.error) {
      if (this.props.FallbackComponent) {
        const FallbackComp = this.props.FallbackComponent
        const props = { error: this.state.error, reset: this.reset, ...this.props.fallbackProps } as FP
        return <FallbackComp {...props} />
      }
      return this.props.fallback || <div>Something went wrong</div>
    }

    return this.props.children
  }
}
