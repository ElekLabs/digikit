/** A folder with a portable document and card data. The firmware path is only a hint. */
export interface SessionDocument {
  schema: 'dk.session/1';
  id: string;
  name: string;
  firmware: {
    path: string;
    sha256: string;
    device: string;
    runtime: boolean;
    dsp: boolean;
    imageSha256: string | null;
  };
  checkpoint: string | null;
  attachments: { name: string; kind: 'capture' | 'audio' | 'comparison'; path: string }[];
  dsp?: { module: string; reportFolder?: string };
}

export interface SavedSession {
  root: string;
  document: SessionDocument;
}

export interface DspReportSource {
  folder: string;
  sha256: string;
}

export interface CreateSession {
  destination: string;
  name: string;
  firmware: string;
  source: 'live' | 'image' | 'folder';
  card?: string;
  fork?: string;
  runtime: boolean;
  dsp: boolean;
}

export interface SessionHost {
  onAction?(receive: (action: string) => void): Promise<() => void>;
  setActionsEnabled?(enabled: boolean): Promise<void>;
  pick(kind: 'open' | 'save' | 'image' | 'folder' | 'attachment', name?: string): Promise<string | null>;
  open(file: string): Promise<SavedSession>;
  create(input: CreateSession): Promise<SavedSession>;
  mount(root: string, firmware?: string): Promise<[session: SavedSession, card: string]>;
  release(card: string): Promise<void>;
  save(root: string, card: string | undefined, name: string): Promise<SavedSession>;
  saveDsp(root: string, module: string, source?: DspReportSource): Promise<SavedSession>;
  preserve(card: string): Promise<void>;
  attach(root: string, source: string): Promise<SavedSession>;
  readCapture(root: string, path: string): Promise<string>;
  openAttachment(root: string, path: string): Promise<void>;
}
