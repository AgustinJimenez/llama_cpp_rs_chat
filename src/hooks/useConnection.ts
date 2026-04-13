import { useContext } from 'react';

import { ConnectionContext } from '../contexts/connectionState';

export const useConnection = () => useContext(ConnectionContext);
