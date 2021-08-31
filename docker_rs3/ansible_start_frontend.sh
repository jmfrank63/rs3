#/bin/bash
ansible-playbook -u ubuntu --private-key ~/.ssh/LightsailDefaultKey-eu-west-2.pem -i frontend.common-work-education.co.uk, playbook_prod.yml
