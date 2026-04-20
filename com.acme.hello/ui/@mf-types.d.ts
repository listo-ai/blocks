
    export type RemoteKeys = 'REMOTE_ALIAS_IDENTIFIER/Panel';
    type PackageType<T> = T extends 'REMOTE_ALIAS_IDENTIFIER/Panel' ? typeof import('REMOTE_ALIAS_IDENTIFIER/Panel') :any;