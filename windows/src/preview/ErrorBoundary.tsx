import React from "react";

type ErrorBoundaryProps = {
  children: React.ReactNode;
  t: (key: string) => string;
  onRetry: () => void;
};

type ErrorBoundaryState = {
  error: Error | null;
};

/**
 * Keep a malformed preview isolated from the history window. Preview
 * renderers consume untrusted, user-controlled bytes and third-party parsers;
 * a parser exception should leave the window usable and offer a bounded retry.
 */
export class PreviewErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    console.error("Preview renderer failed:", error, info.componentStack);
  }

  private retry = () => {
    this.setState({ error: null });
    this.props.onRetry();
  };

  render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="preview-state preview-state-error" role="alert" data-testid="preview-renderer-error">
        <h2>{this.props.t("history.preview.rendererError")}</h2>
        <p>{this.props.t("history.preview.rendererErrorDescription")}</p>
        <button type="button" className="preview-primary-button" onClick={this.retry}>
          {this.props.t("history.preview.retry")}
        </button>
      </div>
    );
  }
}
