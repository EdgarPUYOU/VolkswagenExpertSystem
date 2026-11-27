# Contract review rules (ported from VW_Contracts_rules.ttl).
# The comparisons use the volumes stored in the TTL. When the numbers move to SQLite
# (plan.md step 8) these two rules move into the Rust services.

PREFIX vw: <http://example.org/volkswagen/>

# ContractRule1: projected volume exceeds the contracted commitment
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?c a vw:ContractReviewCandidate ;
       vw:contractReviewReason vw:VolumeExceedsCommitment .
    ?comp a vw:RenegotiationCandidateComponent .
  }
}
WHERE {
  ?c a vw:SupplyContract ;
     vw:contractCoversComponent ?comp ;
     vw:projectedAnnualComponentVolume ?projected ;
     vw:currentContractedAnnualVolume ?committed .
  FILTER(?projected > ?committed)
} ;

# ContractRule2: projected volume reaches a discount tier that has a lower unit price
INSERT {
  GRAPH <http://example.org/volkswagen/graph/inferred> {
    ?c a vw:ContractReviewCandidate ;
       vw:contractReviewReason vw:VolumeDiscountAvailable .
    ?comp a vw:RenegotiationCandidateComponent .
  }
}
WHERE {
  ?c a vw:SupplyContract ;
     vw:contractCoversComponent ?comp ;
     vw:projectedAnnualComponentVolume ?projected ;
     vw:discountTierAnnualVolume ?tier ;
     vw:currentUnitPriceEUR ?price ;
     vw:discountUnitPriceEUR ?discount .
  FILTER(?projected >= ?tier && ?discount < ?price)
}
