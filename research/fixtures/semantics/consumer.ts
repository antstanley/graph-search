import {shared as renamed, unique} from './a';
function imported() { renamed(); unique(); }
function arbitrary() { external.unique(); }
function chained() { factory().unique(); }
function factory() { return {}; }
